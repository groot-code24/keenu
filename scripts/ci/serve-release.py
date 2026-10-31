#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Serves a folder of packages as a release on this computer, so CI can run the one-command installer before anything
is published. Usage: serve-release.py <folder> <port> <platform> <kind>"""
import functools
import hashlib
import http.server
import json
import pathlib
import sys


def main():
    folder, port, platform, kind = pathlib.Path(sys.argv[1]), int(sys.argv[2]), sys.argv[3], sys.argv[4]
    package = next(path for path in sorted(folder.iterdir()) if path.name.endswith(f".{kind}"))
    (folder / "keenu-release.json").write_text(json.dumps({
        "product": "Keenu",
        "schema": 1,
        "releases": [{
            "version": "0.0.0-test",
            "channel": "prerelease",
            "artifacts": [{
                "platform": platform,
                "kind": kind,
                "file_name": package.name,
                "url": f"http://127.0.0.1:{port}/{package.name}",
                "sha256": hashlib.sha256(package.read_bytes()).hexdigest(),
                "size": package.stat().st_size,
                "min_os": "",
                "signature": {"kind": "none", "signed": False, "signer": ""},
            }],
        }],
    }), encoding="utf-8")
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(folder))
    http.server.ThreadingHTTPServer(("127.0.0.1", port), handler).serve_forever()


main()
