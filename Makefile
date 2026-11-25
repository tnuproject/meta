# Development shortcuts. Portable to BSD make and GNU make.

CARGO ?= cargo
PYTHON ?= python3
SHELLCHECK ?= shellcheck
BINDIR ?= target/debug

SCRIPTS = scripts/build-kernel.sh tests/ota-e2e.sh packages/native/build-pkg.sh \
	packages/debian/build-deb.sh packages/arch/build-arch.sh

all: build

build:
	$(CARGO) build --workspace --locked

test:
	$(CARGO) test --workspace --locked

lint:
	$(CARGO) fmt --all --check
	$(CARGO) clippy --workspace --all-targets --locked -- -D warnings

docs:
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps --locked
	$(PYTHON) scripts/check-docs.py

data:
	$(PYTHON) scripts/check-data.py

shell:
	$(SHELLCHECK) $(SCRIPTS)

ota: build
	sh tests/ota-e2e.sh $(BINDIR)

check: lint test docs data ota

clean:
	$(CARGO) clean

.PHONY: all build test lint docs data shell ota check clean
