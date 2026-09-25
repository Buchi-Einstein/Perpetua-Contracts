#!/usr/bin/env python3
"""Audit Soroban storage-key discriminants for collisions.

Every contracttype storage-key variant must carry an explicit, unique tag.
Implicit Rust enum ordering is rejected because inserting a variant can move
existing serialized keys and make old state inaccessible or ambiguous.
"""

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

ENUM_START = re.compile(
    r"#\[contracttype\](?:(?:\s*#\[[^\]]+\])*)\s*"
    r"pub\s+enum\s+(DataKey|Key)\s*\{",
    re.MULTILINE,
)
VARIANT = re.compile(
    r"^\s*([A-Za-z_]\w*)\s*(?:\([^\n]*\))?\s*"
    r"(?:=\s*(\d+))?\s*,?\s*$",
    re.MULTILINE,
)


def _enum_body(source: str, opening_brace: int) -> str:
    depth = 1
    index = opening_brace
    while depth and index + 1 < len(source):
        index += 1
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
    if depth:
        raise ValueError("unterminated storage-key enum")
    return source[opening_brace + 1 : index]


def parse_key_enums(path: Path) -> list[tuple[str, list[tuple[str, int | None]]]]:
    """Return contracttype storage-key enums and their declared tags."""
    source = path.read_text(encoding="utf-8")
    enums = []
    for match in ENUM_START.finditer(source):
        body = _enum_body(source, match.end() - 1)
        variants = []
        for line in body.splitlines():
            line = re.sub(r"^\s*///?.*$", "", line)
            variant = VARIANT.match(line)
            if variant:
                tag = variant.group(2)
                variants.append((variant.group(1), int(tag) if tag else None))
        enums.append((match.group(1), variants))
    return enums


def main() -> int:
    files = sorted((REPO_ROOT / "contracts").glob("*/src/**/*.rs"))
    storage_enums = []
    for path in files:
        for enum_name, variants in parse_key_enums(path):
            storage_enums.append((path, enum_name, variants))

    failed = False

    for path, enum_name, variants in storage_enums:
        missing = [name for name, tag in variants if tag is None]
        tags = {}
        for name, tag in variants:
            if tag is not None:
                tags.setdefault(tag, []).append(name)
        collisions = {tag: names for tag, names in tags.items() if len(names) > 1}
        if missing:
            print(f"IMPLICIT TAG in {path}:{enum_name}: {missing}")
            failed = True
        if collisions:
            print(f"COLLISION in {path}:{enum_name}: {collisions}")
            failed = True

    if not storage_enums:
        print("FAIL: No contracttype storage-key enums found.")
        return 1

    if failed:
        print("FAIL: Storage-key discriminant issues detected.")
        return 1

    print(f"OK: Checked {len(storage_enums)} storage-key enums.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
