CARGO ?= cargo
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
DESTDIR ?=

.PHONY: all build release test check install clean

all: build

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test

check:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings

install: release
	install -Dm755 target/release/wallfetch "$(DESTDIR)$(BINDIR)/wallfetch"

clean:
	$(CARGO) clean
