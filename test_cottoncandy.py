import importlib.machinery
import importlib.util
import os

import pytest

SCRIPT = os.path.join(os.path.dirname(__file__), "cottoncandy")


def load_module():
    loader = importlib.machinery.SourceFileLoader("cottoncandy_module", SCRIPT)
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


cottoncandy = load_module()


@pytest.fixture
def store():
    database = cottoncandy.Store(":memory:")
    yield database
    database.close()


def test_parse_arguments_defaults():
    options = cottoncandy.parse_arguments([])
    assert options.urls == []
    assert options.new_window is False
    assert options.private is False
    assert options.set_default is False


def test_parse_arguments_options_and_urls():
    options = cottoncandy.parse_arguments(
        ["cottoncandy", "--private", "--new-window", "example.org", "https://a.test"]
    )
    assert options.private is True
    assert options.new_window is True
    assert options.urls == ["example.org", "https://a.test"]


def test_parse_arguments_rejects_chromium_switches():
    # Chromium would read --no-sandbox from argv; argparse must refuse it.
    with pytest.raises(SystemExit):
        cottoncandy.parse_arguments(["--no-sandbox"])


@pytest.mark.parametrize(
    "text, expected",
    [
        ("https://example.org/a?b=c", "https://example.org/a?b=c"),
        ("example.org", "https://example.org"),
        ("www.example.org/path#x", "https://www.example.org/path#x"),
        ("localhost:8000/app", "http://localhost:8000/app"),
        ("127.0.0.1", "http://127.0.0.1"),
        ("[::1]:3000", "http://[::1]:3000"),
        ("xn--bcher-kva.example", "https://xn--bcher-kva.example"),
        ("file:///tmp/a.html", "file:///tmp/a.html"),
        ("about:blank", "about:blank"),
        ("mailto:me@example.org", "mailto:me@example.org"),
        ("  example.org  ", "https://example.org"),
    ],
)
def test_resolve_input_opens_addresses(text, expected):
    assert cottoncandy.resolve_input(text) == expected


@pytest.mark.parametrize(
    "text, query",
    [
        ("grace os browser", "grace+os+browser"),
        ("python", "python"),
        ("javascript:alert(1)", "javascript%3Aalert%281%29"),
        ("data:text/html,<b>x</b>", "data%3Atext%2Fhtml%2C%3Cb%3Ex%3C%2Fb%3E"),
        ("what is a.b", "what+is+a.b"),
        ("1+1", "1%2B1"),
    ],
)
def test_resolve_input_searches_everything_else(text, query):
    assert cottoncandy.resolve_input(text) == "https://duckduckgo.com/?q=" + query


def test_resolve_input_empty():
    assert cottoncandy.resolve_input("") is None
    assert cottoncandy.resolve_input("   ") is None
    assert cottoncandy.resolve_input(None) is None


def test_argument_to_url(tmp_path):
    page = tmp_path / "My Page.html"
    page.write_text("<p>hi</p>")
    assert cottoncandy.argument_to_url("My Page.html", str(tmp_path)) == page.resolve().as_uri()
    assert cottoncandy.argument_to_url(str(page), "/") == page.resolve().as_uri()
    assert cottoncandy.argument_to_url("https://example.org", "/") == "https://example.org"
    assert cottoncandy.argument_to_url("example.org", "/nowhere") == "https://example.org"
    assert cottoncandy.argument_to_url("javascript:alert(1)", "/") is None
    assert cottoncandy.argument_to_url("steam://run/1", "/") is None
    assert cottoncandy.argument_to_url("", "/") is None


@pytest.mark.parametrize(
    "url, main_frame, typed, clicked, expected",
    [
        ("https://example.org", True, False, False, "allow"),
        ("http://example.org", False, False, False, "allow"),
        ("about:blank", False, False, False, "allow"),
        ("data:text/html,x", False, False, False, "allow"),
        ("data:text/html,x", True, False, True, "block"),
        ("blob:https://example.org/1", True, False, False, "allow"),
        ("file:///etc/passwd", True, True, False, "allow"),
        ("chrome://sandbox", True, True, False, "allow"),
        ("chrome://sandbox", True, False, True, "block"),
        ("mailto:a@example.org", True, False, True, "handoff"),
        ("mailto:a@example.org", True, False, False, "block"),
        ("mailto:a@example.org", False, False, True, "block"),
        ("steam://run/1", True, False, True, "block"),
        ("ms-msdt:/id", True, False, True, "block"),
        ("", True, True, False, "block"),
    ],
)
def test_navigation_decision(url, main_frame, typed, clicked, expected):
    assert (
        cottoncandy.navigation_decision(url, main_frame, typed=typed, user_initiated=clicked)
        == expected
    )


@pytest.mark.parametrize(
    "suggested, expected",
    [
        ("report.pdf", "report.pdf"),
        ("../../.bashrc", "bashrc"),
        ("..\\..\\evil.exe", "evil.exe"),
        ("/etc/passwd", "passwd"),
        (".hidden", "hidden"),
        ("a\x00b\nc.txt", "abc.txt"),
        ("", "download"),
        (None, "download"),
        ("...", "download"),
    ],
)
def test_safe_download_name(suggested, expected):
    assert cottoncandy.safe_download_name(suggested) == expected


def test_safe_download_name_keeps_extension_when_long():
    name = cottoncandy.safe_download_name("x" * 300 + ".tar.gz")
    assert len(name) == 200
    assert name.endswith(".gz")


def test_unique_path(tmp_path):
    directory = str(tmp_path)
    assert cottoncandy.unique_path(directory, "a.zip") == os.path.join(directory, "a.zip")
    (tmp_path / "a.zip").write_text("")
    assert cottoncandy.unique_path(directory, "a.zip") == os.path.join(directory, "a (1).zip")
    taken = {os.path.join(directory, "a (1).zip")}
    assert cottoncandy.unique_path(directory, "a.zip", taken) == os.path.join(directory, "a (2).zip")


def test_format_bytes():
    assert cottoncandy.format_bytes(0) == "0 B"
    assert cottoncandy.format_bytes(999) == "999 B"
    assert cottoncandy.format_bytes(1500) == "1.5 KB"
    assert cottoncandy.format_bytes(2_500_000) == "2.5 MB"
    assert cottoncandy.format_bytes(7_000_000_000_000) == "7000.0 GB"
    assert cottoncandy.format_bytes(-1) == "unknown size"


def test_permission_description_denies_unknown():
    assert cottoncandy.permission_description("MediaVideoCapture") == "use your camera"
    assert cottoncandy.permission_description("LocalFontsAccess") is None
    assert cottoncandy.permission_description("Unsupported") is None


def test_window_title():
    assert cottoncandy.window_title("Docs") == "Docs — CottonCandy"
    assert cottoncandy.window_title("", private=True) == "CottonCandy (Private)"
    assert cottoncandy.window_title(None) == "CottonCandy"


def test_grace_accent():
    assert cottoncandy.grace_accent("accent = pink   # comment") == "pink"
    assert cottoncandy.grace_accent("accent = pink\naccent=teal\n") == "teal"
    assert cottoncandy.grace_accent("accent = neon") == "blue"
    assert cottoncandy.grace_accent("") == "blue"
    assert cottoncandy.grace_accent("# accent = red") == "blue"


def test_set_default_handlers_new_file():
    text = cottoncandy.set_default_handlers("")
    assert text.startswith("[Default Applications]\n")
    for mime in cottoncandy.HANDLER_TYPES:
        assert "{}=org.graceos.CottonCandy.desktop".format(mime) in text


def test_set_default_handlers_keeps_other_entries():
    original = (
        "# mine\n"
        "[Added Associations]\n"
        "text/html=firefox.desktop;\n"
        "\n"
        "[Default Applications]\n"
        "x-scheme-handler/http=firefox.desktop\n"
        "image/png=org.gnome.Loupe.desktop\n"
        "\n"
        "[Removed Associations]\n"
        "text/plain=foo.desktop\n"
    )
    text = cottoncandy.set_default_handlers(original)
    lines = text.splitlines()
    assert "# mine" in lines
    assert "text/html=firefox.desktop;" in lines  # Added Associations is left alone
    assert "image/png=org.gnome.Loupe.desktop" in lines
    assert "x-scheme-handler/http=firefox.desktop" not in lines
    assert "text/plain=foo.desktop" in lines
    defaults = lines[lines.index("[Default Applications]") : lines.index("[Removed Associations]")]
    assert "x-scheme-handler/https=org.graceos.CottonCandy.desktop" in defaults
    assert cottoncandy.set_default_handlers(text) == text  # running it twice changes nothing


def test_make_default_browser(tmp_path):
    path = cottoncandy.make_default_browser({"XDG_CONFIG_HOME": str(tmp_path / "config")})
    with open(path, encoding="utf-8") as handle:
        assert "x-scheme-handler/http=org.graceos.CottonCandy.desktop" in handle.read()


def test_paths_follow_xdg():
    environ = {"XDG_DATA_HOME": "/d", "XDG_CACHE_HOME": "/c", "XDG_CONFIG_HOME": "/f"}
    assert cottoncandy.data_directory(environ) == "/d/cottoncandy"
    assert cottoncandy.cache_directory(environ) == "/c/cottoncandy"
    assert cottoncandy.config_home(environ) == "/f"


def test_runtime_socket_path(tmp_path):
    assert cottoncandy.runtime_socket_path({"XDG_RUNTIME_DIR": str(tmp_path)}) == str(tmp_path / "cottoncandy.sock")
    fallback = cottoncandy.runtime_socket_path({}, uid=1000)
    assert fallback.endswith("cottoncandy-1000.sock")


def test_request_round_trip():
    data = cottoncandy.encode_request(["example.org"], "/home/me", new_window=True)
    assert cottoncandy.decode_request(data) == {
        "urls": ["example.org"],
        "cwd": "/home/me",
        "new_window": True,
        "private": False,
    }


@pytest.mark.parametrize(
    "data",
    [
        b"",
        b"not json",
        b"\xff\xfe",
        b"[]",
        b'{"urls": "example.org", "cwd": "/"}',
        b'{"urls": [1], "cwd": "/"}',
        b'{"urls": [], "cwd": 5}',
        b'{"urls": [' + b",".join([b'"a"'] * 101) + b'], "cwd": "/"}',
        b" " * (cottoncandy.MAX_REQUEST_BYTES + 1),
    ],
)
def test_decode_request_rejects_malformed(data):
    assert cottoncandy.decode_request(data) is None


def test_decode_request_needs_literal_true():
    data = b'{"urls": [], "cwd": "/", "private": "yes", "new_window": 1}'
    request = cottoncandy.decode_request(data)
    assert request["private"] is False
    assert request["new_window"] is False


def test_icon_markup_and_stylesheet():
    assert 'stroke="#ffffff"' in cottoncandy.icon_markup("M0 0", "#ffffff")
    assert 'fill="#ffffff"' in cottoncandy.icon_markup("M0 0", "#ffffff", filled=True)
    assert cottoncandy.ACCENTS["pink"]["fg"] in cottoncandy.stylesheet("pink")
    assert cottoncandy.ACCENTS["blue"]["fg"] in cottoncandy.stylesheet("unknown")


def test_store_records_visits(store):
    assert store.record_visit("https://example.org/", "Example", now=10)
    assert store.record_visit("https://example.org/", "", now=20)
    assert not store.record_visit("about:blank", "Blank")
    assert not store.record_visit("chrome://gpu", "GPU")
    rows = store.history()
    assert rows == [("https://example.org/", "Example", 20)]


def test_store_history_search_escapes_wildcards(store):
    store.record_visit("https://a.test/100%25", "Sale 100%", now=1)
    store.record_visit("https://b.test/", "Other", now=2)
    assert [row[0] for row in store.history("100%")] == ["https://a.test/100%25"]
    assert store.history("_") == []
    assert [row[0] for row in store.history()] == ["https://b.test/", "https://a.test/100%25"]


def test_store_suggestions_rank_bookmarks_then_visits(store):
    store.record_visit("https://docs.python.org/", "Python docs", now=1)
    for stamp in range(3):
        store.record_visit("https://python.org/", "Python", now=10 + stamp)
    store.add_bookmark("https://pypi.org/", "PyPI python packages", now=5)
    assert store.suggestions("python") == [
        "https://pypi.org/",
        "https://python.org/",
        "https://docs.python.org/",
    ]
    assert store.suggestions("") == []
    assert store.suggestions("python", limit=1) == ["https://pypi.org/"]


def test_store_bookmarks(store):
    assert store.add_bookmark("https://example.org/", "Example", now=1)
    assert not store.add_bookmark("javascript:alert(1)", "Nope")
    assert store.is_bookmarked("https://example.org/")
    store.add_bookmark("https://example.org/", "Renamed", now=2)
    assert store.bookmarks() == [("https://example.org/", "Renamed", 1)]
    store.remove_bookmark("https://example.org/")
    assert not store.is_bookmarked("https://example.org/")


def test_store_set_title_and_clear(store):
    store.record_visit("https://example.org/", "", now=1)
    store.add_bookmark("https://example.org/", "", now=1)
    store.set_title("https://example.org/", "Example")
    assert store.history()[0][1] == "Example"
    assert store.bookmarks()[0][1] == "Example"
    store.clear_history()
    assert store.history() == []
    assert store.is_bookmarked("https://example.org/")


def test_store_file_is_private(tmp_path):
    path = tmp_path / "data" / "cottoncandy.db"
    database = cottoncandy.Store(str(path))
    database.record_visit("https://example.org/", "Example", now=1)
    database.close()
    assert oct(os.stat(path).st_mode & 0o777) == "0o600"
    assert oct(os.stat(path.parent).st_mode & 0o777) == "0o700"
    reopened = cottoncandy.Store(str(path))
    assert reopened.history() == [("https://example.org/", "Example", 1)]
    reopened.close()


def test_main_refuses_root(monkeypatch, capsys):
    monkeypatch.setattr(os, "geteuid", lambda: 0)
    assert cottoncandy.main([]) == 1
    assert "root" in capsys.readouterr().err


def test_main_set_default(monkeypatch, tmp_path, capsys):
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
    assert cottoncandy.main(["--set-default"]) == 0
    assert (tmp_path / "mimeapps.list").exists()
    assert "x-scheme-handler/https" in capsys.readouterr().out


def test_is_local_host():
    for host in ("localhost", "127.0.0.1", "127.8.9.10", "[::1]", "::1", "app.localhost"):
        assert cottoncandy.is_local_host(host)
    for host in ("example.org", "10.0.0.1", "127.example.org", "localhost.example.org", ""):
        assert not cottoncandy.is_local_host(host)


def test_themes_define_the_same_tokens():
    assert set(cottoncandy.THEMES["dark"]) == set(cottoncandy.THEMES["light"])
    for accent in cottoncandy.ACCENTS.values():
        assert set(accent) == {"base", "fill", "fg", "fg_light", "tint"}


def test_grace_style():
    assert cottoncandy.grace_style("style = light   # dark | light") == "light"
    assert cottoncandy.grace_style("style = dark\nstyle = light") == "light"
    assert cottoncandy.grace_style("style = sepia") == "dark"
    assert cottoncandy.grace_style("") == "dark"


def test_mix():
    assert cottoncandy.mix("#000000", "#ffffff", 0.5) == "#808080"
    assert cottoncandy.mix("#2f75cb", "#000000", 0) == "#2f75cb"
    assert cottoncandy.mix("#2f75cb", "#000000", 1) == "#000000"


def test_theme_tokens_pick_accent_text_per_style():
    dark = cottoncandy.theme_tokens("dark", "pink")
    light = cottoncandy.theme_tokens("light", "pink")
    assert dark["accent_text"] == cottoncandy.ACCENTS["pink"]["fg"]
    assert light["accent_text"] == cottoncandy.ACCENTS["pink"]["fg_light"]
    assert dark["accent_fill"] == light["accent_fill"] == cottoncandy.ACCENTS["pink"]["fill"]
    assert dark["tint"] == cottoncandy.ACCENTS["pink"]["tint"]
    assert light["window"] == "#ffffff"
    # Hover darkens the fill so the white label stays above 4.5:1 (lightening fails, see the design spec).
    assert dark["accent_fill_hover"] == cottoncandy.mix(cottoncandy.ACCENTS["pink"]["fill"], "#000000", 0.10)
    assert cottoncandy.theme_tokens("neon", "nope")["style"] == "dark"


def test_stylesheet_uses_the_theme():
    light = cottoncandy.stylesheet("teal", "light")
    assert cottoncandy.THEMES["light"]["titlebar"] in light
    assert cottoncandy.ACCENTS["teal"]["fg_light"] in light
    assert cottoncandy.THEMES["dark"]["titlebar"] not in light


@pytest.mark.parametrize(
    "url, kind, host, rest",
    [
        ("https://wiki.archlinux.org/title/Hyprland", "secure", "wiki.archlinux.org", "/title/Hyprland"),
        ("https://example.org/", "secure", "example.org", ""),
        ("http://old.example.org/login?x=1", "insecure", "old.example.org", "/login?x=1"),
        ("http://127.0.0.1:8765/", "local", "127.0.0.1:8765", ""),
        ("http://localhost:3000/app", "local", "localhost:3000", "/app"),
        ("http://user@localhost/", "local", "user@localhost", ""),
        ("file:///home/me/a.html", "file", "file://", "/home/me/a.html"),
        ("cottoncandy://newtab", "empty", "", ""),
        ("cottoncandy://newtab/", "empty", "", ""),
        ("about:blank", "empty", "", ""),
        ("", "empty", "", ""),
        ("chrome://sandbox/", "other", "chrome://sandbox/", ""),
    ],
)
def test_url_parts(url, kind, host, rest):
    assert cottoncandy.url_parts(url) == {"kind": kind, "host": host, "rest": rest}


def test_format_speed_and_eta():
    assert cottoncandy.format_speed(18_000_000) == "18.0 MB/s"
    assert cottoncandy.format_speed(0) == ""
    assert cottoncandy.format_eta(42) == "42 s left"
    assert cottoncandy.format_eta(0.2) == "1 s left"
    assert cottoncandy.format_eta(150) == "2 min left"
    assert cottoncandy.format_eta(3 * 3600 + 120) == "3 h 2 min left"
    assert cottoncandy.format_eta(None) == ""


def test_build_suggestions_groups_and_order():
    rows = cottoncandy.build_suggestions(
        "py",
        open_tabs=[("https://docs.python.org/3/", "Python docs"), ("cottoncandy://newtab", "New Tab")],
        bookmarks=[("https://pypi.org/", "PyPI"), ("https://docs.python.org/3/", "Python docs")],
        history=[("https://python.org/", "Python"), ("https://example.org/", "Example")],
    )
    assert [(r["group"], r["url"]) for r in rows] == [
        ("Open tabs", "https://docs.python.org/3/"),
        ("Bookmarks", "https://pypi.org/"),
        ("History", "https://python.org/"),
        ("Search", "https://duckduckgo.com/?q=py"),
    ]
    assert rows[0]["kind"] == "tab" and rows[-1]["kind"] == "search"


def test_build_suggestions_offers_typed_address_first_in_search():
    rows = cottoncandy.build_suggestions("example.org")
    assert [(r["kind"], r["url"]) for r in rows] == [
        ("address", "https://example.org"),
        ("search", "https://duckduckgo.com/?q=example.org"),
    ]
    assert cottoncandy.build_suggestions("   ") == []


def test_error_kind():
    assert cottoncandy.error_kind("CertificateErrorDomain", -202) == "cert"
    assert cottoncandy.error_kind("ConnectionErrorDomain", -106) == "offline"
    assert cottoncandy.error_kind("DnsErrorDomain", -105) == "dns"
    assert cottoncandy.error_kind("HttpErrorDomain", -105) == "dns"
    assert cottoncandy.error_kind("ConnectionErrorDomain", -102) == "other"


def test_internal_request():
    assert cottoncandy.internal_request("cottoncandy://newtab") == ("newtab", None)
    assert cottoncandy.internal_request("cottoncandy://newtab/") == ("newtab", None)
    assert cottoncandy.internal_request("cottoncandy://search?q=grace+os") == ("search", "grace os")
    assert cottoncandy.internal_request("cottoncandy://search") == ("search", "")
    assert cottoncandy.internal_request("cottoncandy://back") == ("back", None)
    assert cottoncandy.internal_request("cottoncandy://settings") == (None, None)
    assert cottoncandy.internal_request("https://newtab") == (None, None)


def test_internal_scheme_is_typed_only():
    assert cottoncandy.navigation_decision("cottoncandy://newtab", True, typed=True) == "allow"
    assert cottoncandy.navigation_decision("cottoncandy://newtab", True, user_initiated=True) == "block"
    assert cottoncandy.resolve_input("cottoncandy://newtab") == "cottoncandy://newtab"


def test_newtab_html_escapes_and_locks_down():
    tokens = cottoncandy.theme_tokens("light", "blue")
    html = cottoncandy.newtab_html(
        tokens,
        shortcuts=[("https://evil.example/\"><script>x</script>", "<b>Evil</b>", None)],
        recent=[("https://a.example/", "A & B", "data:image/png;base64,AAAA")],
    )
    assert "<script>" not in html
    assert "&lt;b&gt;Evil&lt;/b&gt;" in html
    assert "A &amp; B" in html
    assert "default-src 'none'" in html and "script-src" not in html
    assert 'action="cottoncandy://search"' in html
    assert tokens["tint"] in html
    private = cottoncandy.newtab_html(tokens, shortcuts=[("https://a.example/", "A", None)], private=True)
    assert "private window" in private and "a.example" not in private


def test_error_html():
    tokens = cottoncandy.theme_tokens("dark", "blue")
    cert = cottoncandy.error_html(tokens, "cert", "https://bank.example/<x>", "ERR_CERT_DATE_INVALID")
    assert "never lets you continue" in cert
    assert "Try again" not in cert  # no way past a bad certificate
    assert "<x>" not in cert and "bank.example" in cert
    offline = cottoncandy.error_html(tokens, "offline", "https://wiki.archlinux.org/x", "ERR_INTERNET_DISCONNECTED")
    assert "You’re offline" in offline and 'href="https://wiki.archlinux.org/x"' in offline
    other = cottoncandy.error_html(tokens, "other", "http://127.0.0.1:9/", "Connection refused", "Connection refused")
    assert "Can’t reach 127.0.0.1:9" in other
    assert "<script" not in other


def test_store_icons_top_sites_and_matches(store):
    for stamp in range(3):
        store.record_visit("https://docs.python.org/3/", "Python docs", now=stamp)
    store.record_visit("https://docs.python.org/3/library/", "Library", now=5)
    store.record_visit("https://example.org/", "Example", now=6)
    store.record_visit("file:///tmp/a.html", "A file", now=7)
    assert store.top_sites() == [("https://docs.python.org/3/", "Python docs"), ("https://example.org/", "Example")]
    assert store.history_matches("python")[0] == ("https://docs.python.org/3/", "Python docs")
    store.add_bookmark("https://pypi.org/", "PyPI", now=1)
    assert store.bookmark_matches("pyp") == [("https://pypi.org/", "PyPI")]
    assert store.bookmark_matches("%") == []
    assert store.set_icon("https://docs.python.org/3/", b"\x89PNG data")
    assert store.icon_for("https://docs.python.org/anything") == b"\x89PNG data"
    assert not store.set_icon("file:///tmp/a.html", b"png")
    assert not store.set_icon("https://big.example/", b"x" * (300 * 1024))
    assert store.icon_for("https://nothing.example/") is None
