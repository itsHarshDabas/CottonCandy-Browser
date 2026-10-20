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
