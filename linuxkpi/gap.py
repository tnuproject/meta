#!/usr/bin/env python3
"""Finds Linux kernel interfaces a driver uses that LinuxKPI lacks.

usage: gap.py FREEBSD_SOURCE DRIVER_DIRECTORY [--baseline DRIVER]... [--json]

DRIVER_DIRECTORY is relative to the FreeBSD source tree. The analysis is textual:
it compares the headers a driver includes and the functions it calls with what
LinuxKPI and the FreeBSD kernel declare. It cannot judge whether an interface that
exists behaves like Linux's, so a driver without gaps is not a working driver.
See docs/compatibility/linuxkpi.md for how to read the result.
"""

import argparse
import collections
import json
import pathlib
import re
import sys

KEYWORDS = set(
    "if for while switch return sizeof typeof __typeof__ defined do else case alignof "
    "__alignof__ offsetof asm __asm__ __attribute__ _Static_assert static_assert "
    "__builtin_expect va_start va_end va_arg va_copy".split()
)
COMMENT = re.compile(r"/\*.*?\*/|//[^\n]*", re.DOTALL)
STRING = re.compile(r'"(?:\\.|[^"\\\n])*"|\'(?:\\.|[^\'\\\n])*\'')
INCLUDE = re.compile(r"^\s*#\s*include\s*<([^>]+)>", re.MULTILINE)
CALL = re.compile(r"(?<![\w.>])([a-z_][a-z0-9_]{2,})\s*\(")
MACRO = re.compile(r"^\s*#\s*define\s+(\w+)", re.MULTILINE)
IDENTIFIER = re.compile(r"\b[A-Za-z_]\w*\b")
# Where LinuxKPI marks something as not really implemented.
STUB = re.compile(r"\b(TODO|UNIMPLEMENTED|IMPROVE_TODO)\b")
FUNCTION = re.compile(r"^(?:[\w\s*]*?\b)?(\w+)\s*\([^;{]*\)\s*\{(.*?)^\}", re.MULTILINE | re.DOTALL)
LOCAL = re.compile(r"^(?:\w[\w\s*]*?[\s*])?(\w+)\s*\([^;{}]*\)\s*\{", re.MULTILINE)
LINUX_HEADERS = ("linux/", "net/", "asm/", "asm-generic/", "uapi/", "acpi/", "crypto/", "drm/")


def read(path):
    return path.read_text(encoding="utf-8", errors="replace")


def sources(directory):
    for suffix in ("*.c", "*.h"):
        yield from sorted(directory.rglob(suffix))


def provided_by_kernel(source):
    """Identifiers declared by LinuxKPI and the parts of the kernel it builds on."""
    names = set()
    stubs = set()
    kpi = source / "sys/compat/linuxkpi"
    for path in sources(kpi):
        text = read(path)
        names.update(IDENTIFIER.findall(STRING.sub("", COMMENT.sub("", text))))
        for name, body in FUNCTION.findall(text):
            if STUB.search(body):
                stubs.add(name)
        stubs.update(re.findall(r"^\s*#\s*define\s+(\w+)\([^)]*\)[^\n]*\b(?:TODO|UNIMPLEMENTED)\b",
                                text, re.MULTILINE))
    for directory in ("sys/sys", "sys/net", "sys/vm", "sys/libkern", "sys/net80211"):
        for path in (source / directory).rglob("*.h"):
            names.update(IDENTIFIER.findall(COMMENT.sub("", read(path))))
    return names, stubs


def headers(source):
    common = source / "sys/compat/linuxkpi/common/include"
    dummy = source / "sys/compat/linuxkpi/dummy/include"
    real = {p.relative_to(common).as_posix() for p in common.rglob("*.h")}
    empty = {p.relative_to(dummy).as_posix() for p in dummy.rglob("*.h")}
    return real, empty


def analyze(source, driver):
    provided, stubs = provided_by_kernel(source)
    real, empty = headers(source)
    included = collections.Counter()
    called = collections.Counter()
    own = set()
    files = lines = 0
    for path in sources(driver):
        text = read(path)
        files += 1
        lines += text.count("\n")
        included.update(h for h in INCLUDE.findall(text) if h.startswith(LINUX_HEADERS))
        code = STRING.sub('""', COMMENT.sub("", text))
        own.update(MACRO.findall(code))
        own.update(LOCAL.findall(code))
        # Structure members and declarations, such as callbacks.
        own.update(re.findall(r"\(\s*\*\s*(\w+)\s*\)\s*\(", code))
        own.update(re.findall(r"^\w[\w\s*]*?[\s*](\w+)\s*\([^;{}]*\)\s*;", code, re.MULTILINE))
        called.update(CALL.findall(code))
    # Names the driver's own headers declare elsewhere in the same family.
    family = driver.parent
    if family.name not in ("dev", "contrib"):
        for path in family.glob("*.h"):
            code = COMMENT.sub("", read(path))
            own.update(MACRO.findall(code))
            own.update(IDENTIFIER.findall(code))
    used = {name: count for name, count in called.items()
            if name not in KEYWORDS and name not in own}
    missing = {name: count for name, count in used.items() if name not in provided}
    stubbed = {name: count for name, count in used.items() if name in stubs}
    return {
        "files": files,
        "lines": lines,
        "interfaces": len(used),
        "missing": missing,
        "stubbed": stubbed,
        "headers_missing": sorted(h for h in included if h not in real and h not in empty),
        "headers_empty": sorted(h for h in included if h in empty and h not in real),
    }


def ranked(counts):
    return sorted(counts.items(), key=lambda item: (-item[1], item[0]))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("source", type=pathlib.Path)
    parser.add_argument("driver")
    parser.add_argument("--json", action="store_true")
    parser.add_argument(
        "--baseline", action="append", default=[], metavar="DRIVER",
        help="a driver that works; what it lacks too is conditional code, not a gap",
    )
    options = parser.parse_args()
    driver = options.source / options.driver
    if not (options.source / "sys/compat/linuxkpi").is_dir() or not driver.is_dir():
        sys.exit("gap.py: not a FreeBSD source tree, or no such driver")
    result = analyze(options.source, driver)
    for baseline in options.baseline:
        known = analyze(options.source, options.source / baseline)
        for key in ("missing", "stubbed"):
            result[key] = {n: c for n, c in result[key].items() if n not in known[key]}
        result["headers_missing"] = [
            h for h in result["headers_missing"] if h not in known["headers_missing"]
        ]
    if options.json:
        json.dump(result, sys.stdout, indent=2)
        print()
        return
    print(f"{options.driver}: {result['files']} files, {result['lines']} lines, "
          f"{result['interfaces']} kernel interfaces used")
    for title, key in (("missing", "missing"), ("present but marked unfinished", "stubbed")):
        items = ranked(result[key])
        print(f"\n{title}: {len(items)}")
        for name, count in items:
            print(f"  {count:4}  {name}")
    print(f"\nheaders missing: {', '.join(result['headers_missing']) or 'none'}")
    print(f"headers that are empty placeholders: {len(result['headers_empty'])}")


if __name__ == "__main__":
    main()
