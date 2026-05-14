BINARY     := ane
INSTALL_PATH ?= /usr/local/bin
# Honour CARGO_TARGET_DIR if set in the environment. Falls back to the cargo
# default of `target`.
TARGET_DIR := $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target)

.PHONY: all build install test clean release publish

all: build

build:
	cargo build --release

install: build
	install -m 755 $(TARGET_DIR)/release/$(BINARY) $(INSTALL_PATH)/$(BINARY)

test:
	cargo test
	cargo clippy -- -D warnings
	cargo fmt --check

clean:
	cargo clean

release:
	@if [ -z "$(VERSION)" ]; then \
		echo "Usage: make release VERSION=vx.y.z"; \
		exit 1; \
	fi
	@bash scripts/release.sh "$(VERSION)"

publish:
	cargo publish
