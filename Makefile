# make install [PREFIX=/usr] [DESTDIR=...]   |   make check
PREFIX ?= /usr
APP_ID = org.graceos.CottonCandy

install:
	install -Dm755 cottoncandy "$(DESTDIR)$(PREFIX)/bin/cottoncandy"
	install -Dm644 $(APP_ID).desktop "$(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop"
	install -Dm644 $(APP_ID).svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg"

uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/cottoncandy" \
	      "$(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop" \
	      "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg"

# Unit tests need no Qt; the browser itself is checked by hand (see README).
check:
	python -m pytest -q test_cottoncandy.py
	desktop-file-validate $(APP_ID).desktop

.PHONY: install uninstall check
