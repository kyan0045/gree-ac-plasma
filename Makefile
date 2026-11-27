PREFIX ?= /usr/local
DESTDIR ?=

BINDIR := $(PREFIX)/bin
SYSTEMD_USER_DIR := $(PREFIX)/lib/systemd/user
PLASMOID_DIR := $(PREFIX)/share/plasma/plasmoids/com.greeac.ac
LICENSE_DIR := $(PREFIX)/share/licenses/gree-ac-plasma

PLASMOID_SRC := plasmoid/com.greeac.ac
PLASMOID_FILES := $(shell find $(PLASMOID_SRC) -type f | sort)

.PHONY: build install uninstall

build:
	cargo build --release --locked

install:
	install -Dm755 target/release/gree-ac-plasma "$(DESTDIR)$(BINDIR)/gree-ac-plasma"
	install -Dm644 /dev/stdin "$(DESTDIR)$(SYSTEMD_USER_DIR)/gree-ac-plasma.service" < <(sed 's|ExecStart=/usr/local/bin/gree-ac-plasma|ExecStart=$(BINDIR)/gree-ac-plasma|' packaging/gree-ac-plasma.service)
	@for f in $(PLASMOID_FILES); do \
		rel="$${f#$(PLASMOID_SRC)/}"; \
		install -Dm644 "$$f" "$(DESTDIR)$(PLASMOID_DIR)/$$rel"; \
	done
	install -Dm644 LICENSE "$(DESTDIR)$(LICENSE_DIR)/LICENSE"

uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/gree-ac-plasma"
	rm -f "$(DESTDIR)$(SYSTEMD_USER_DIR)/gree-ac-plasma.service"
	rm -rf "$(DESTDIR)$(PLASMOID_DIR)"
	rm -rf "$(DESTDIR)$(LICENSE_DIR)"
