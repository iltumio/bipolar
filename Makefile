PREFIX ?= $(HOME)/.local

.PHONY: build release check install
build:
	cargo build --locked
release:
	cargo build --release --locked
check:
	cargo fmt --all --check
	cargo clippy --locked --all-targets -- -D warnings
	cargo test --locked
install: release
	install -Dm755 target/release/bipolar "$(DESTDIR)$(PREFIX)/bin/bipolar"
	install -Dm644 data/dev.iltumio.Bipolar.desktop "$(DESTDIR)$(PREFIX)/share/applications/dev.iltumio.Bipolar.desktop"
	install -Dm644 data/dev.iltumio.Bipolar.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/dev.iltumio.Bipolar.svg"
