.PHONY: migrate help fmt sqlx-prepare check build test verify update lint

# Ensure commands work even when rustup has no default toolchain set.
# Users can override, e.g. `make check-server TOOLCHAIN=nightly`.
TOOLCHAIN ?= stable
CARGO ?= cargo

help:
	@echo "Available targets:"
	@echo "  fmt          - Format all crates"
	@echo "  sqlx-prepare - Update sqlx workspace metadata"
	@echo "  check        - Run cargo check on all crates"
	@echo "  build        - Build all crates"
	@echo "  test         - Run tests"
	@echo "  update       - Update the dependency lockfile"
	@echo "  lint         - Run Clippy on all targets and features"
	@echo "  verify       - Run formatting and all checks"

fmt:
	$(CARGO) +$(TOOLCHAIN) fmt

sqlx-prepare:
	$(CARGO) +$(TOOLCHAIN) sqlx prepare --workspace

check:
	$(CARGO) +$(TOOLCHAIN) check

build:
	$(CARGO) +$(TOOLCHAIN) build

test:
	$(CARGO) +$(TOOLCHAIN) test $(TEST_ARGS)

verify: fmt sqlx-prepare lint build test

# Apply schema changes before SQLx checks against a development database.
migrate:
	$(CARGO) +$(TOOLCHAIN) sqlx migrate run --source runelink-server/migrations

update:
	$(CARGO) +$(TOOLCHAIN) update

lint:
	$(CARGO) +$(TOOLCHAIN) clippy $(CLIPPY_ARGS) --workspace --all-targets --all-features -- -D warnings
