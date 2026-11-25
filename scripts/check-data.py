#!/usr/bin/env python3
"""Validates data files: TOML, JSON, XML and FreeBSD kernel configuration names.

Uses only the Python standard library (3.11 or newer).
"""

import json
import pathlib
import sys
import tomllib
import xml.etree.ElementTree

ROOT = pathlib.Path(__file__).resolve().parent.parent
SKIP = {".git", "target", ".claude", "dist", "build", "node_modules"}


def files():
    for path in sorted(ROOT.rglob("*")):
        relative = path.relative_to(ROOT)
        if path.is_file() and not SKIP.intersection(relative.parts):
            yield path, relative


def main():
    errors = []
    counts = {}
    for path, relative in files():
        name = path.name
        try:
            if name.endswith((".toml", ".toml.example")) or name == "Cargo.lock":
                kind = "TOML"
                tomllib.loads(path.read_text(encoding="utf-8"))
            elif name.endswith(".json"):
                kind = "JSON"
                json.loads(path.read_text(encoding="utf-8"))
            elif name.endswith((".xml", ".policy")) or path.parent.name == "dbus" and name.endswith(".conf"):
                kind = "XML"
                xml.etree.ElementTree.parse(path)
            else:
                continue
        except (ValueError, xml.etree.ElementTree.ParseError) as error:
            errors.append(f"{relative.as_posix()}: {error}")
            continue
        counts[kind] = counts.get(kind, 0) + 1
    for error in errors:
        print(error, file=sys.stderr)
    print(", ".join(f"{count} {kind}" for kind, count in sorted(counts.items())), "files checked")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
