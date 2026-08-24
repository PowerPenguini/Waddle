SHELL := /bin/sh

CARGO ?= cargo
INSTALL ?= install
PREFIX ?= /usr
DESTDIR ?=
CARGO_TARGET_DIR ?= target
BINARY ?= $(CARGO_TARGET_DIR)/release/polarexp

APP_ID := io.github.powerpenguini.PolarExp
BINDIR := $(PREFIX)/bin
DATADIR := $(PREFIX)/share
APPLICATIONSDIR := $(DATADIR)/applications
METAINFODIR := $(DATADIR)/metainfo
ICONDIR := $(DATADIR)/icons/hicolor/scalable/apps

.PHONY: all build check install uninstall refresh-desktop-caches

all: build

build:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --release --locked

check:
	desktop-file-validate data/$(APP_ID).desktop
	appstreamcli validate --pedantic --no-net data/$(APP_ID).metainfo.xml
	test -x "$(BINARY)"

install: check
	$(INSTALL) -Dm0755 "$(BINARY)" "$(DESTDIR)$(BINDIR)/polarexp"
	$(INSTALL) -Dm0644 "data/$(APP_ID).desktop" \
		"$(DESTDIR)$(APPLICATIONSDIR)/$(APP_ID).desktop"
	$(INSTALL) -Dm0644 "data/$(APP_ID).metainfo.xml" \
		"$(DESTDIR)$(METAINFODIR)/$(APP_ID).metainfo.xml"
	$(INSTALL) -Dm0644 "data/icons/hicolor/scalable/apps/$(APP_ID).svg" \
		"$(DESTDIR)$(ICONDIR)/$(APP_ID).svg"
	$(MAKE) refresh-desktop-caches

uninstall:
	rm -f -- "$(DESTDIR)$(BINDIR)/polarexp"
	rm -f -- "$(DESTDIR)$(APPLICATIONSDIR)/$(APP_ID).desktop"
	rm -f -- "$(DESTDIR)$(METAINFODIR)/$(APP_ID).metainfo.xml"
	rm -f -- "$(DESTDIR)$(ICONDIR)/$(APP_ID).svg"
	$(MAKE) refresh-desktop-caches

refresh-desktop-caches:
	@if [ -z "$(DESTDIR)" ]; then \
		if command -v update-desktop-database >/dev/null 2>&1; then \
			update-desktop-database "$(APPLICATIONSDIR)"; \
		fi; \
		if command -v gtk-update-icon-cache >/dev/null 2>&1; then \
			gtk-update-icon-cache --force --ignore-theme-index "$(DATADIR)/icons/hicolor"; \
		fi; \
	fi
