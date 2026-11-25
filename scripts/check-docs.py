#!/usr/bin/env python3
"""Checks documentation.

- Relative links and heading anchors resolve.
- README.md's status table agrees with status.toml.
- Only the five status labels appear in status tables.
- The project is never called by the name it must not be called.
- Text files are UTF-8 with LF line endings and no trailing whitespace.

Uses only the Python standard library (3.11 or newer).
"""

import pathlib
import re
import sys
import tomllib
import unicodedata

ROOT = pathlib.Path(__file__).resolve().parent.parent
SKIP = {".git", "target", ".claude", "dist", "build", "fixtures"}
LABELS = {"IMPLEMENTED", "EXPERIMENTAL", "IN DEVELOPMENT", "PLANNED", "RESEARCH REQUIRED"}
TEXT_SUFFIXES = {
    ".md", ".rs", ".serial", ".toml", ".yml", ".sh", ".py", ".xml", ".in", ".jsonc", ".txt", ".example",
}
# Written so that this file does not contain the forbidden name itself.
FORBIDDEN = re.compile("META" + r"[ -]OS\b")

LINK = re.compile(r"(?<!!)\[[^\]]*\]\(([^)\s]+)\)")
FENCE = re.compile(r"^(```|~~~).*?^\1", re.MULTILINE | re.DOTALL)


def text_files():
    for path in sorted(ROOT.rglob("*")):
        relative = path.relative_to(ROOT)
        if not path.is_file() or SKIP.intersection(relative.parts):
            continue
        if path.suffix in TEXT_SUFFIXES or path.name in {"Makefile", "PKGBUILD", "LICENSE"}:
            yield path, relative.as_posix()


def slug(heading):
    """GitHub's heading anchor."""
    heading = re.sub(r"[`*_]", "", heading.strip().lower())
    heading = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", heading)
    kept = [c for c in heading if c in " -" or unicodedata.category(c)[0] in "LN"]
    return "".join(kept).replace(" ", "-")


def anchors(path):
    text = FENCE.sub("", path.read_text(encoding="utf-8"))
    return {slug(match) for match in re.findall(r"^#+ (.+)$", text, re.MULTILINE)}


def check_links(path, name, text, errors):
    for target in LINK.findall(FENCE.sub("", text)):
        if re.match(r"[a-z]+:", target):
            continue
        file_part, _, anchor = target.partition("#")
        destination = (path.parent / file_part).resolve() if file_part else path
        if not destination.exists():
            errors.append(f"{name}: broken link {target}")
        elif anchor and destination.suffix == ".md" and anchor not in anchors(destination):
            errors.append(f"{name}: no heading for {target}")


def check_status_tables(name, text, errors):
    """Any table with a 'Status' column may use only the five labels there."""
    column = None
    for line in text.splitlines():
        if not line.startswith("|"):
            column = None
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if column is None:
            column = cells.index("Status") if "Status" in cells else -1
        elif column >= 0 and column < len(cells) and not set(cells[column]) <= set("-: "):
            if cells[column] not in LABELS:
                errors.append(f"{name}: unknown status {cells[column]!r}")


def check_readme_status(errors):
    components = tomllib.loads((ROOT / "status.toml").read_text(encoding="utf-8"))["component"]
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    rows = dict(re.findall(r"^\| ([^|]+?) \| ([A-Z ]+?) \|", readme, re.MULTILINE))
    for component in components:
        found = rows.get(component["name"])
        if found != component["status"]:
            errors.append(
                f"README.md: {component['name']} should be {component['status']}, found {found}"
            )


def check_encoding(name, raw, errors):
    if b"\r" in raw:
        errors.append(f"{name}: carriage returns")
    if raw and not raw.endswith(b"\n"):
        errors.append(f"{name}: no newline at end of file")
    for number, line in enumerate(raw.split(b"\n"), 1):
        if line.endswith((b" ", b"\t")):
            errors.append(f"{name}:{number}: trailing whitespace")


def main():
    errors = []
    documents = 0
    for path, name in text_files():
        raw = path.read_bytes()
        check_encoding(name, raw, errors)
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError as error:
            errors.append(f"{name}: {error}")
            continue
        if FORBIDDEN.search(text):
            errors.append(f"{name}: the project is called META")
        if path.suffix == ".md":
            documents += 1
            check_links(path, name, text, errors)
            check_status_tables(name, text, errors)
    check_readme_status(errors)
    for error in errors:
        print(error, file=sys.stderr)
    print(f"{documents} documents checked, {len(errors)} problems")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
