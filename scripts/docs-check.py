#!/usr/bin/env python3
"""Check documentation links and lightweight metadata."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
STATUS_WORDS = re.compile(
    r"\b("
    r"proposed|active|accepted|implemented|landed|deferred|future|historical|"
    r"superseded|deprecated|withdrawn|rejected|closed|complete|done|planned|"
    r"partially|current|in progress"
    r")\b",
    re.IGNORECASE,
)
ADR_ID = re.compile(r"^(\d{4})-.+\.md$")
ADR_VERSION = re.compile(r"^\d+\.\d+\.\d+$")
PLAN_REQUIRED = ("## Scope", "## Non-goals", "## Slices", "## Gates", "## Verification")
ADR_REQUIRED = (
    "- Status:",
    "- Version:",
    "- Date:",
    "## Context",
    "## Decision",
    "## Consequences",
    "## Verification",
)

errors: list[str] = []


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        errors.append(f"{path.relative_to(ROOT)}: not UTF-8")
        return ""


def check_links(path: Path, text: str) -> None:
    for match in re.finditer(r"\[[^\]]*\]\(([^)]+)\)", text):
        target = match.group(1).split("#", 1)[0].strip()
        if not target or "://" in target or target.startswith(("mailto:", "data:")):
            continue
        if not (path.parent / target).resolve().exists():
            errors.append(f"{path.relative_to(ROOT)}: missing link target {target}")


def status_block(text: str) -> str | None:
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if line.startswith("Status:") or line.startswith("- Status:"):
            return " ".join(lines[index : index + 3]).casefold()
    return None


def check_status(path: Path, text: str) -> None:
    block = status_block(text)
    if block is None:
        errors.append(f"{path.relative_to(ROOT)}: missing Status line")
    elif not STATUS_WORDS.search(block):
        errors.append(f"{path.relative_to(ROOT)}: unknown Status value")


def check_adrs() -> None:
    seen: dict[str, Path] = {}
    for path in sorted((ROOT / "docs/adr").glob("[0-9][0-9][0-9][0-9]-*.md")):
        match = ADR_ID.match(path.name)
        if not match:
            errors.append(f"{path.relative_to(ROOT)}: invalid ADR file name")
            continue
        number = match.group(1)
        if number in seen:
            errors.append(
                f"{path.relative_to(ROOT)}: duplicate ADR number {number} "
                f"(also {seen[number].relative_to(ROOT)})"
            )
        seen[number] = path
        text = read(path)
        check_status(path, text)
        for line in text.splitlines():
            if line.startswith("- Version:"):
                value = line.split(":", 1)[1].strip()
                if not ADR_VERSION.fullmatch(value):
                    errors.append(
                        f"{path.relative_to(ROOT)}: invalid ADR Version {value!r}"
                    )
                break
        if not any(line.startswith("- Date:") for line in text.splitlines()):
            errors.append(f"{path.relative_to(ROOT)}: missing Date")


def check_plans() -> None:
    for path in sorted((ROOT / "docs").glob("*plan*.md")):
        check_status(path, read(path))


def check_templates() -> None:
    plan = ROOT / "docs/templates/plan.md"
    adr = ROOT / "docs/templates/adr.md"
    if not plan.exists():
        errors.append("docs/templates/plan.md: missing")
    else:
        text = read(plan)
        for heading in PLAN_REQUIRED:
            if heading not in text:
                errors.append(f"docs/templates/plan.md: missing {heading}")
    if not adr.exists():
        errors.append("docs/templates/adr.md: missing")
    else:
        text = read(adr)
        for required in ADR_REQUIRED:
            if required not in text:
                errors.append(f"docs/templates/adr.md: missing {required}")


def main() -> int:
    markdown = sorted(ROOT.rglob("*.md"))
    for path in markdown:
        check_links(path, read(path))
    check_adrs()
    check_plans()
    check_templates()
    if errors:
        print("\n".join(errors))
        return 1
    print(f"docs-check: ok ({len(markdown)} Markdown files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
