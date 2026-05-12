PREFIX ?= /usr/local
BINARY = ane

.PHONY: build test install clean

build:
	cargo build

test:
	cargo test
	cargo clippy -- -D warnings
	cargo fmt --check

install:
	cargo build --release
	install -d $(DESTDIR)$(PREFIX)/bin
	install -m 755 target/release/$(BINARY) $(DESTDIR)$(PREFIX)/bin/$(BINARY)

clean:
	cargo clean
