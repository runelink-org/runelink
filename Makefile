.PHONY: check check-server fmt build build-server test help

# Ensure commands work even when rustup has no default toolchain set.
# Users can override, e.g. `make check-server TOOLCHAIN=nightly`.
TOOLCHAIN ?= stable
CARGO ?= cargo

help:
	@echo "Available targets:"
	@echo "  fmt          - Format all crates"
	@echo "  check        - Run cargo check on all crates"
	@echo "  build        - Build all crates"
	@echo "  test         - Run tests"

fmt:
	$(CARGO) +$(TOOLCHAIN) fmt

check:
	$(CARGO) +$(TOOLCHAIN) check

build:
	$(CARGO) +$(TOOLCHAIN) build

test:
	$(CARGO) +$(TOOLCHAIN) test
