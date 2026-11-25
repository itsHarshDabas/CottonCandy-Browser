#!/usr/bin/env python3
"""Measures a browser's startup time and memory on Linux, so CottonCandy's numbers are measured, not promised.

For each run the browser starts with a fresh profile and N tabs of local test pages. Each page reports to
this script's web server when it has loaded. We record:

- first page: seconds from launch until the first page has loaded;
- all pages: seconds from launch until every tab has loaded;
- memory: the proportional set size (PSS) of every process the browser started, five seconds after the last
  page loaded. PSS counts shared memory once across processes, so the sum is what the browser really uses.

Example:
    bench.py --name CottonCandy --tabs 5 -- ./cottoncandy {urls}
    bench.py --name Chrome --tabs 5 -- google-chrome --user-data-dir={profile} --no-first-run {urls}

{urls} becomes the page addresses and {profile} a fresh empty folder. HOME is also set to a fresh folder.
Only standard-library Python; Linux only (it reads /proc).
"""

import argparse
import http.server
import json
import os
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import threading
import time

PAGE = """<!doctype html><html><head><meta charset="utf-8"><title>Bench {n}</title>
<style>body{{font:15px sans-serif;margin:0;padding:24px;background:#fafafa}} .card{{display:inline-block;width:180px;
height:120px;margin:6px;border-radius:12px;background:linear-gradient(135deg,#6366f1,#ec4899);color:#fff;padding:10px;
box-sizing:border-box}} table{{border-collapse:collapse}} td{{border:1px solid #ddd;padding:3px 6px}}</style></head>
<body><h1>Benchmark page {n}</h1><div id="cards"></div><table id="t"></table>
<script>
// A page with some work in it: a few hundred elements, a table, a little script.
const cards = document.getElementById("cards");
for (let i = 0; i < 120; i++) {{ const d = document.createElement("div"); d.className = "card"; d.textContent = "Card " + i; cards.appendChild(d); }}
const t = document.getElementById("t");
for (let r = 0; r < 200; r++) {{ const tr = t.insertRow(); for (let c = 0; c < 8; c++) tr.insertCell().textContent = (r * c).toString(16); }}
let x = 0; for (let i = 0; i < 2e6; i++) x += Math.sqrt(i);
window.addEventListener("load", () => fetch("/loaded?n={n}&x=" + Math.round(x)));
</script></body></html>"""


class Server(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self):
        super().__init__(("127.0.0.1", 0), Handler)
        self.loaded = {}
        self.lock = threading.Lock()


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        path, _, query = self.path.partition("?")
        if path == "/loaded":
            n = dict(p.split("=", 1) for p in query.split("&") if "=" in p).get("n", "?")
            with self.server.lock:
                self.server.loaded.setdefault(n, time.monotonic())
            body, kind = b"ok", "text/plain"
        elif path.startswith("/page/"):
            body, kind = PAGE.format(n=path.rsplit("/", 1)[-1]).encode(), "text/html"
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)


def session_pids(sid):
    """Every process in the browser's session (it was started with setsid)."""
    pids = []
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        try:
            with open(f"/proc/{name}/stat") as f:
                fields = f.read().rsplit(")", 1)[1].split()
            if int(fields[3]) == sid:  # field 6, session id, counted after the ")"
                pids.append(int(name))
        except (OSError, IndexError, ValueError):
            pass
    return pids


def pss_kb(pid):
    try:
        with open(f"/proc/{pid}/smaps_rollup") as f:
            for line in f:
                if line.startswith("Pss:"):
                    return int(line.split()[1])
    except OSError:
        pass
    return 0


def one_run(command, tabs, server, timeout, settle):
    base = f"http://127.0.0.1:{server.server_address[1]}/page/"
    urls = [f"{base}{i}" for i in range(tabs)]
    work = tempfile.mkdtemp(prefix="ccbench-")
    profile = os.path.join(work, "profile")
    home = os.path.join(work, "home")
    os.makedirs(profile)
    os.makedirs(home)
    argv = []
    for arg in command:
        if arg == "{urls}":
            argv.extend(urls)
        else:
            argv.append(arg.replace("{profile}", profile))
    env = dict(os.environ, HOME=home, XDG_CONFIG_HOME=os.path.join(home, ".config"),
               XDG_DATA_HOME=os.path.join(home, ".local/share"), XDG_CACHE_HOME=os.path.join(home, ".cache"))
    with server.lock:
        server.loaded.clear()
    start = time.monotonic()
    proc = subprocess.Popen(argv, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    try:
        while time.monotonic() - start < timeout:
            with server.lock:
                done = len(server.loaded)
            if done >= tabs or proc.poll() is not None:
                break
            time.sleep(0.02)
        with server.lock:
            times = sorted(server.loaded.values())
        if len(times) < tabs:
            raise RuntimeError(f"only {len(times)} of {tabs} pages loaded")
        first, last = times[0] - start, times[-1] - start
        time.sleep(settle)
        pids = session_pids(proc.pid)
        memory = sum(pss_kb(p) for p in pids) / 1024
        return {"first_page_s": round(first, 3), "all_pages_s": round(last, 3), "memory_mb": round(memory, 1), "processes": len(pids)}
    finally:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
            proc.wait(timeout=10)
        except (ProcessLookupError, subprocess.TimeoutExpired):
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        shutil.rmtree(work, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--name", required=True)
    parser.add_argument("--tabs", type=int, default=5)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--timeout", type=float, default=90)
    parser.add_argument("--settle", type=float, default=5)
    parser.add_argument("--json", help="append the result as one JSON line to this file")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("give the browser command after --")
    server = Server()
    threading.Thread(target=server.serve_forever, daemon=True).start()
    # A first run warms the disk cache so every browser is measured the same way.
    one_run(command, args.tabs, server, args.timeout, 1)
    runs = [one_run(command, args.tabs, server, args.timeout, args.settle) for _ in range(args.runs)]
    median = {k: statistics.median(r[k] for r in runs) for k in ("first_page_s", "all_pages_s", "memory_mb")}
    result = {"browser": args.name, "tabs": args.tabs, "runs": len(runs), "median": median, "all": runs}
    print(json.dumps(result))
    if args.json:
        with open(args.json, "a") as f:
            f.write(json.dumps(result) + "\n")


if __name__ == "__main__":
    sys.exit(main())
