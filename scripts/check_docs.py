#!/usr/bin/env python3
"""Validate local Markdown links in ZillaOS canonical entrypoints and specs.

No third-party packages, no network requests and no rewriting historic docs.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
ENTRYPOINTS = (
    "README.md",
    "AGENTS.md",
    "docs/README.md",
    "docs/STATUS.md",
    "docs/ARCHITECTURE_CURRENT.md",
    "docs/DEVELOPMENT.md",
    "docs/DOCUMENTATION_POLICY.md",
    "docs/BOOTSTRAP_ADOPTION.md",
    "docs/NEW_CHAT_START_I_M2.md",
    "docs/NEXT_WAVE_I_M2.md",
    "docs/verification/EVIDENCE_MATRIX.md",
)
LINK_PATTERN = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")
FENCE_PATTERN = re.compile(r"^\s*(\x60{3}|~{3})")
SCHEME_PATTERN = re.compile(r"^[A-Za-z][A-Za-z0-9+.-]*:")


def sources() -> list[Path]:
    return ([ROOT / entry for entry in ENTRYPOINTS]
            + sorted((ROOT / "docs/specs").rglob("*.md")))


def check_one(path: Path) -> list[str]:
    if not path.is_file():
        return [f"{path.relative_to(ROOT)}: missing canonical document"]
    issues: list[str] = []
    fenced = False
    for line_num, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if FENCE_PATTERN.match(line):
            fenced = not fenced
            continue
        if fenced:
            continue
        for match in LINK_PATTERN.finditer(line):
            raw = match.group(1).strip()
            if raw.startswith("<"):
                raw = raw[1:].split(">", 1)[0]
            else:
                raw = raw.split()[0]
            if not raw or raw.startswith(("#", "//")) or SCHEME_PATTERN.match(raw):
                continue
            local = unquote(urlsplit(raw).path)
            if not local:
                continue
            if local.startswith("/"):
                issues.append(f"{path.relative_to(ROOT)}:{line_num}: absolute local path {local}")
                continue
            destination = (path.parent / local).resolve()
            try:
                destination.relative_to(ROOT)
            except ValueError:
                issues.append(f"{path.relative_to(ROOT)}:{line_num}: escapes repo {raw}")
                continue
            if not destination.exists():
                issues.append(f"{path.relative_to(ROOT)}:{line_num}: missing file {raw}")
    return issues


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--verbose", action="store_true")
    args = p.parse_args()
    documents = sources()
    issues: list[str] = []
    for doc in documents:
        issues.extend(check_one(doc))
        if args.verbose:
            print(f"scan: {doc.relative_to(ROOT)}")
    if issues:
        print("\n".join(issues), file=sys.stderr)
        print(f"FAIL: {len(issues)} invalid documentation link(s)", file=sys.stderr)
        return 1
    print(f"PASS: {len(documents)} canonical Markdown files, all relative paths resolve")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
