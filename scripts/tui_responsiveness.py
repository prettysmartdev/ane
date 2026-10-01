#!/usr/bin/env python3
"""WI-16 Linux/macOS PTY check; standard library only.

Build: cargo build --release --features test-support --bins
Run: python3 scripts/tui_responsiveness.py --ane target/release/ane \
         --mock-server target/release/mock_lsp_server

Creates a 50k-entry tree, stalls semantic/symbol LSP requests, produces a
filesystem storm, and measures input-to-visible-update latency. Uses a small
ANSI screen reader, rather than treating unrelated periodic output as an
acknowledgement of a key. This is a reference benchmark, not a CI timing test.
"""
import argparse
import codecs
import errno
import fcntl
import json
import os
from pathlib import Path
import platform
import pty
import re
import select
import shlex
import struct
import subprocess
import tempfile
import termios
import threading
import time
import unicodedata


class Screen:
    def __init__(self, rows=24, cols=100):
        self.rows, self.cols = rows, cols
        self.cells = [[(" ", None) for _ in range(cols)] for _ in range(rows)]
        self.row = self.col = 0
        self.bg = None
        self.pending = ""
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")

    def feed(self, data):
        text = self.pending + self.decoder.decode(data)
        self.pending = ""
        i = 0
        while i < len(text):
            if text[i] == "\x1b":
                if i + 1 >= len(text):
                    break
                if text[i + 1] == "[":
                    match = re.match(r"\x1b\[([0-?]*)([ -/]*)([@-~])", text[i:])
                    if not match:
                        break
                    raw, _, code = match.groups()
                    args = [int(x) if x else 0 for x in raw.lstrip("?=>").split(";")]
                    a = args[0] or 1
                    if code in "Hf":
                        self.row = a - 1
                        self.col = (args[1] if len(args) > 1 and args[1] else 1) - 1
                    elif code == "G":
                        self.col = a - 1
                    elif code == "d":
                        self.row = a - 1
                    elif code == "A":
                        self.row -= a
                    elif code == "B":
                        self.row += a
                    elif code == "C":
                        self.col += a
                    elif code == "D":
                        self.col -= a
                    elif code == "J" and args[0] in (2, 3):
                        self.cells = [[(" ", None) for _ in range(self.cols)] for _ in range(self.rows)]
                    elif code == "K" and 0 <= self.row < self.rows:
                        for col in range(max(0, self.col), self.cols):
                            self.cells[self.row][col] = (" ", self.bg)
                    elif code == "m":
                        j = 0
                        while j < len(args):
                            value = args[j]
                            if value in (0, 49):
                                self.bg = None
                            elif 40 <= value <= 47:
                                self.bg = value - 40
                            elif 100 <= value <= 107:
                                self.bg = value - 100 + 8
                            elif value in (38, 48) and j + 2 < len(args):
                                if args[j + 1] == 5:
                                    if value == 48:
                                        self.bg = args[j + 2]
                                    j += 2
                                elif args[j + 1] == 2 and j + 4 < len(args):
                                    if value == 48:
                                        self.bg = tuple(args[j + 2:j + 5])
                                    j += 4
                            j += 1
                    i += match.end()
                    continue
                if text[i + 1] == "]":
                    end = re.search(r"\x07|\x1b\\", text[i + 2:])
                    if not end:
                        break
                    i += 2 + end.end()
                    continue
                i += 2
                continue
            character = text[i]
            if character == "\r":
                self.col = 0
            elif character == "\n":
                self.row += 1
            elif ord(character) >= 32:
                if 0 <= self.row < self.rows and 0 <= self.col < self.cols:
                    self.cells[self.row][self.col] = (character, self.bg)
                self.col += 0 if unicodedata.combining(character) else (2 if unicodedata.east_asian_width(character) in "WF" else 1)
            i += 1
        self.pending = text[i:]

    def text(self):
        return "\n".join("".join(c for c, _ in row) for row in self.cells)

    def selected(self):
        for row in self.cells[1:-1]:
            label = "".join(c for c, bg in row[:45] if bg == 8).strip()
            if label:
                return label
        return ""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ane", type=Path, default=Path("target/release/ane"))
    parser.add_argument("--mock-server", type=Path, default=Path("target/release/mock_lsp_server"))
    parser.add_argument("--groups", type=int, default=100)
    args = parser.parse_args()
    ane, mock = args.ane.resolve(), args.mock_server.resolve()
    assert ane.is_file() and mock.is_file(), "Build both binaries first"
    with tempfile.TemporaryDirectory(prefix="ane-pty-") as root_name, tempfile.TemporaryDirectory(prefix="ane-lsp-") as bin_name:
        root = Path(root_name)
        source = "let value = 123;\n" * 10_000
        (root / "main.rs").write_text(source)
        for group in range(args.groups):
            for directory in range(100):
                leaf = root / "target" / f"group{group:03}" / f"dir{directory:03}"
                leaf.mkdir(parents=True)
                for file in range(4):
                    (leaf / f"file{file}").touch()
        hook = Path(bin_name) / "rust-analyzer"
        hook.write_text(f"#!/bin/sh\nexec {shlex.quote(str(mock))} --ignore-semantic --ignore-symbols --client-request \"$@\"\n")
        hook.chmod(0o755)
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
        original = termios.tcgetattr(slave)
        env = dict(os.environ, PATH=bin_name + os.pathsep + os.environ.get("PATH", ""), TERM="xterm-256color", ANE_TIMINGS="1")
        env.pop("NO_COLOR", None)
        process = subprocess.Popen([str(ane), "main.rs"], cwd=root, env=env, stdin=slave, stdout=slave, stderr=slave)
        screen = Screen()
        raw = bytearray()
        stop = threading.Event()
        storm = None

        def wait(predicate, timeout=5):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if predicate():
                    return
                ready, _, _ = select.select([master], [], [], 0.02)
                if ready:
                    try:
                        data = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            break
                        raise
                    raw.extend(data)
                    screen.feed(data)
            raise AssertionError("Timed out waiting for a visible update; selected=" + repr(screen.selected()) + "\n" + screen.text())

        def key(data, predicate):
            start = time.perf_counter()
            os.write(master, data)
            wait(predicate)
            return (time.perf_counter() - start) * 1000

        try:
            wait(lambda: "main.rs" in screen.text() and "rs:●" in screen.text(), 10)
            start = time.perf_counter()
            os.write(master, b"\x14")
            wait(lambda: "main.rs" in screen.selected())
            open_ms = (time.perf_counter() - start) * 1000

            def generate_storm():
                leaf = root / "target" / "group000" / "dir000"
                i = 0
                while not stop.is_set():
                    path = leaf / f"event-{i % 3000}"
                    path.write_text("changed")
                    path.unlink(missing_ok=True)
                    i += 1

            storm = threading.Thread(target=generate_storm)
            storm.start()
            navigation = []
            for _ in range(20):
                navigation.append(key(b"\x1b[B", lambda: "target" in screen.selected()))
                navigation.append(key(b"\x1b[A", lambda: "main.rs" in screen.selected()))
            key(b"\x05", lambda: "EDIT" in screen.text())
            typing = []
            for column in range(1, 21):
                typing.append(key(b"x", lambda column=column: f"0:{column} |" in screen.text()))
            stop.set()
            storm.join()
            key(b"\x05", lambda: "CHORD" in screen.text())
            key(b"cifn", lambda: "chord running" in screen.text())
            cancel_ms = key(b"\x1b", lambda: "chord cancelled" in screen.text())
            key(b"cifn", lambda: "chord running" in screen.text())
            key(b"\x03", lambda: "Exit ane?" in screen.text())
            start = time.perf_counter()
            os.write(master, b"\x03")
            wait(lambda: b"\x1b[?1049l" in raw, 3)
            restore_ms = (time.perf_counter() - start) * 1000
            process.wait(timeout=3)
            after = termios.tcgetattr(slave)
            assert after[3] & (termios.ICANON | termios.ECHO) == original[3] & (termios.ICANON | termios.ECHO)
            assert process.returncode == 0
            p95 = lambda values: sorted(values)[int(0.95 * (len(values) - 1))]
            result = dict(os=platform.system(), arch=platform.machine(), source_lines=10_000,
                          files=args.groups * 400 + 1, directories=args.groups * 101 + 1,
                          first_tree_visible_ms=round(open_ms, 2),
                          tree_navigation_p95_ms=round(p95(navigation), 2),
                          typing_p95_ms=round(p95(typing), 2),
                          max_input_ms=round(max(navigation + typing), 2),
                          cancel_ms=round(cancel_ms, 2), terminal_restore_ms=round(restore_ms, 2),
                          timings_file=f"{tempfile.gettempdir()}/ane-timings-{process.pid}.jsonl")
            print(json.dumps(result, indent=2))
            assert max(navigation + typing) < 250, result
            assert p95(navigation + typing) < 100, result
        finally:
            stop.set()
            if storm:
                storm.join(timeout=3)
            if process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    main()
