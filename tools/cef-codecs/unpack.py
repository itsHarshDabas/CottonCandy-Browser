#!/usr/bin/env python3
"""Lays out a CEF minimal distribution the way the `cef` crate expects (what export-cef-dir does):

    unpack.py <cef_binary_..._minimal.tar.bz2> <CEF_PATH>

Release/ and Resources/ go into one folder with include/, libcef_dll/, cmake/, CMakeLists.txt and
CREDITS.html beside them, plus the archive.json the crate checks the version against.
"""
import hashlib
import json
import os
import shutil
import sys
import tarfile
import tempfile


def main(archive, dest):
    name = os.path.basename(archive)
    top = name[: -len(".tar.bz2")]
    sha1 = hashlib.sha1()
    with open(archive, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            sha1.update(chunk)
    if os.path.exists(dest):
        shutil.rmtree(dest)
    os.makedirs(dest)
    with tempfile.TemporaryDirectory(dir=os.path.dirname(os.path.abspath(dest))) as tmp:
        with tarfile.open(archive, "r:bz2") as tar:
            tar.extractall(tmp, filter="tar")
        src = os.path.join(tmp, top)
        for sub in ("Release", "Resources"):
            for entry in os.listdir(os.path.join(src, sub)):
                shutil.move(os.path.join(src, sub, entry), os.path.join(dest, entry))
        for entry in ("include", "libcef_dll", "cmake", "CMakeLists.txt", "CREDITS.html"):
            shutil.move(os.path.join(src, entry), os.path.join(dest, entry))
    with open(os.path.join(dest, "archive.json"), "w") as f:
        json.dump({"type": "minimal", "name": name, "sha1": sha1.hexdigest()}, f, indent=2)
    print(f"{name} -> {dest}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(*sys.argv[1:])
