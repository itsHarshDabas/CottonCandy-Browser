#!/usr/bin/env python3
"""One stage of building CEF with the H.264/AAC (MP4) codecs on free GitHub runners.

A full Chromium build takes far longer than the 6 hours a GitHub job may run, so the workflow
(.github/workflows/cef-codecs.yml) chains jobs: each one restores the previous job's work folder,
builds until its deadline, then saves the folder for the next one. Ninja records every finished
step as it goes, so a stopped build carries on where it left off.

    stage.py <work folder> <deadline, Unix seconds> [--checkout]

It runs CEF's own build script (automate-git.py, from the CEF branch we use) for exactly the CEF
version the `cef` crate expects, with the codecs turned on. Exit code: 0 when the CEF binary
distribution is ready, 75 when the deadline came first (run the next stage), anything else on error.
"""
import os
import signal
import subprocess
import sys
import time
import urllib.request

CEF_BRANCH = "8078"  # Chromium 156.0.8078.100
CEF_COMMIT = "e603d97"  # CEF 156.0.31, what cef = "=156.7.0" is built against
AUTOMATE = f"https://raw.githubusercontent.com/chromiumembedded/cef/{CEF_COMMIT}/tools/automate/automate-git.py"

# CEF's release settings, plus the codecs. symbol_level=0 keeps the build small enough for a
# runner's disk; use_siso=false builds with ninja, whose log makes an interrupted build resumable.
GN_DEFINES = "is_official_build=true proprietary_codecs=true ffmpeg_branding=Chrome symbol_level=0 use_siso=false"
WINDOWS = sys.platform == "win32"
if sys.platform.startswith("linux"):
    # Build against Chromium's Debian sysroot, not the runner's own libraries (see the workflow).
    GN_DEFINES += " use_sysroot=true"


def automate_args(work, checkout):
    script = os.path.join(work, "automate-git.py")
    if not os.path.exists(script):
        urllib.request.urlretrieve(AUTOMATE, script)
    args = [
        sys.executable, script,
        f"--download-dir={work}",
        f"--branch={CEF_BRANCH}",
        f"--checkout={CEF_COMMIT}",
        "--x64-build",
        "--no-debug-build",
        "--build-target=cefsimple",
        "--minimal-distrib-only",
        "--no-distrib-docs",
        "--no-chromium-history",
        # Official builds are optimised with Chromium's profiles (PGO), as the stock CEF builds are.
        "--with-pgo-profiles",
    ]
    if checkout:
        # Fetch everything, generate the build files, build nothing yet.
        return args + ["--no-build", "--no-distrib"]
    return args + ["--no-update", "--force-build"]


def run_until(args, deadline):
    env = dict(os.environ, GN_DEFINES=GN_DEFINES, CEF_ARCHIVE_FORMAT="tar.bz2")
    if WINDOWS:
        env["DEPOT_TOOLS_WIN_TOOLCHAIN"] = "0"
        proc = subprocess.Popen(args, env=env, creationflags=subprocess.CREATE_NEW_PROCESS_GROUP)
    else:
        proc = subprocess.Popen(args, env=env, start_new_session=True)
    while proc.poll() is None:
        if time.time() >= deadline:
            print("::notice::Deadline reached; stopping the build to hand over to the next stage.", flush=True)
            stop(proc)
            return 75
        time.sleep(15)
    return proc.returncode


def stop(proc):
    """Ask the build to stop (ninja then ends its running steps cleanly), and force it if it won't."""
    try:
        if WINDOWS:
            proc.send_signal(signal.CTRL_BREAK_EVENT)
        else:
            os.killpg(proc.pid, signal.SIGINT)
        proc.wait(timeout=300)
    except Exception:
        pass
    if proc.poll() is None:
        if WINDOWS:
            subprocess.run(["taskkill", "/T", "/F", "/PID", str(proc.pid)])
        else:
            os.killpg(proc.pid, signal.SIGKILL)
        proc.wait()


def main():
    work, deadline = os.path.abspath(sys.argv[1]), float(sys.argv[2])
    os.makedirs(work, exist_ok=True)
    checkout = "--checkout" in sys.argv[3:]
    code = run_until(automate_args(work, checkout), deadline)
    if code == 0 and not checkout:
        dist = os.path.join(work, "chromium", "src", "cef", "binary_distrib")
        found = [f for f in os.listdir(dist) if f.endswith("_minimal.tar.bz2")] if os.path.isdir(dist) else []
        if not found:
            print("The build finished but made no minimal distribution.", file=sys.stderr)
            return 1
        print("Built:", ", ".join(found))
    return code


if __name__ == "__main__":
    sys.exit(main())
