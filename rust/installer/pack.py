#!/usr/bin/env python3
"""Builds the one-file installer: the setup program with the browser folder appended to it.

    python3 pack.py <cottoncandy-setup(.exe)> <browser folder> <output>

The layout is: the setup program, then a zip of the folder, then 16 bytes: where the zip starts
(little-endian u64) and the marker CCSETUP1. The setup program finds its files from that footer,
and the uninstaller it writes is the same program cut off where the zip starts.
"""
import os
import struct
import sys
import zipfile


def main(setup, folder, out):
    with open(setup, "rb") as f:
        stub = f.read()
    tmp = out + ".zip"
    with zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for root, dirs, files in os.walk(folder):
            dirs.sort()
            for name in sorted(files):
                path = os.path.join(root, name)
                rel = os.path.relpath(path, folder).replace(os.sep, "/")
                info = zipfile.ZipInfo.from_file(path, rel)
                info.compress_type = zipfile.ZIP_DEFLATED
                with open(path, "rb") as src:
                    z.writestr(info, src.read(), compresslevel=9)
    with open(out, "wb") as f:
        f.write(stub)
        with open(tmp, "rb") as z:
            while chunk := z.read(1 << 20):
                f.write(chunk)
        f.write(struct.pack("<Q", len(stub)) + b"CCSETUP1")
    os.remove(tmp)
    if os.name != "nt":
        os.chmod(out, 0o755)
    print(f"{out}: {os.path.getsize(out) / 1e6:.1f} MB")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(*sys.argv[1:])
