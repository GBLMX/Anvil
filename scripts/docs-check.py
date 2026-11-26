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

ROOT_DOC_LIMIT = 25
ANCHOR_CACHE: dict[Path, set[str]] = {}

errors: list[str] = []


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        errors.append(f"{path.relative_to(ROOT)}: not UTF-8")
        return ""


def heading_slug(text: str) -> str:
    text = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"[`*_~]", "", text)
    text = text.strip().lower()
    text = re.sub(r"[^\w\s-]", "", text)
    return re.sub(r"\s+", "-", text)


def file_anchors(path: Path) -> set[str]:
    resolved = path.resolve()
    if resolved in ANCHOR_CACHE:
        return ANCHOR_CACHE[resolved]

    anchors: set[str] = set()
    seen: dict[str, int] = {}
    fenced = False
    for line in read(path).splitlines():
        if re.match(r"^\s*(```|~~~)", line):
            fenced = not fenced
            continue
        if fenced:
            continue
        match = re.match(r"^(#{1,6})\s+(.*?)\s*#*\s*$", line)
        if not match:
            continue
        slug = heading_slug(match.group(2))
        count = seen.get(slug, 0)
        anchors.add(slug if count == 0 else f"{slug}-{count}")
        seen[slug] = count + 1

    ANCHOR_CACHE[resolved] = anchors
    return anchors


def check_links(path: Path, text: str) -> None:
    for match in re.finditer(r"\[[^\]]*\]\(([^)]+)\)", text):
        raw = match.group(1).strip()
        if not raw or "://" in raw or raw.startswith(("mailto:", "data:")):
            continue
        target, separator, anchor = raw.partition("#")
        target = target.strip()
        target_path = path if not target else (path.parent / target).resolve()
        if not target_path.exists():
            errors.append(f"{path.relative_to(ROOT)}: missing link target {raw}")
            continue
        if separator and anchor and target_path.suffix.lower() == ".md":
            if anchor not in file_anchors(target_path):
                errors.append(
                    f"{path.relative_to(ROOT)}: missing anchor #{anchor} "
                    f"in {target_path.relative_to(ROOT)}"
                )


def status_block(text: str) -> str | None:
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if line.startswith(("Status:", "- Status:")):
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
    for path in sorted((ROOT / "docs/src/adr").glob("[0-9][0-9][0-9][0-9]-*.md")):
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
    for path in sorted((ROOT / "docs/src").glob("*-plan.md")):
        check_status(path, read(path))


def check_summary() -> None:
    """Keep the mdBook chapter list in sync with the files on disk."""
    src = ROOT / "docs/src"
    summary = src / "SUMMARY.md"
    if not summary.exists():
        errors.append("docs/src/SUMMARY.md: missing")
        return
    listed: list[Path] = []
    for match in re.finditer(r"^\s*[-*]\s+\[[^\]]*\]\(([^)]+)\)", read(summary), re.MULTILINE):
        target = (src / match.group(1).split("#", 1)[0]).resolve()
        if not target.is_file():
            errors.append(f"docs/src/SUMMARY.md: missing chapter {match.group(1)}")
            continue
        listed.append(target)
    seen: set[Path] = set()
    for path in listed:
        if path in seen:
            errors.append(f"docs/src/SUMMARY.md: duplicate chapter {path.relative_to(ROOT)}")
        seen.add(path)
    for path in sorted(src.rglob("*.md")):
        if path.name == "SUMMARY.md":
            continue
        if path not in seen:
            errors.append(f"{path.relative_to(ROOT)}: not listed in docs/src/SUMMARY.md")

def check_budget() -> None:
    """Keep active documentation bounded and plans out of the root."""
    src = ROOT / "docs/src"
    root_docs = sorted(src.glob("*.md"))
    if len(root_docs) > ROOT_DOC_LIMIT:
        errors.append(
            f"docs/src: {len(root_docs)} root Markdown files "
            f"exceeds budget {ROOT_DOC_LIMIT}"
        )
    for path in root_docs:
        if path.name.endswith("-plan.md"):
            errors.append(
                f"{path.relative_to(ROOT)}: plans must live under "
                "plans/active, plans/proposed, or history/plans"
            )
    for path in sorted(src.rglob("*-plan.md")):
        rel = path.relative_to(src)
        allowed = (
            (rel.parts[0] == "templates")
            or (rel.parts[:2] == ("history", "plans"))
            or (rel.parts[0] == "plans" and rel.parts[1] in {"active", "proposed"})
        )
        if not allowed:
            errors.append(
                f"{path.relative_to(ROOT)}: plan is outside the allowed plan locations"
            )


def check_templates() -> None:
    plan = ROOT / "docs/src/templates/plan.md"
    adr = ROOT / "docs/src/templates/adr.md"
    if not plan.exists():
        errors.append("docs/src/templates/plan.md: missing")
    else:
        text = read(plan)
        for heading in PLAN_REQUIRED:
            if heading not in text:
                errors.append(f"docs/src/templates/plan.md: missing {heading}")
    if not adr.exists():
        errors.append("docs/src/templates/adr.md: missing")
    else:
        text = read(adr)
        for required in ADR_REQUIRED:
            if required not in text:
                errors.append(f"docs/src/templates/adr.md: missing {required}")


def main() -> int:
    markdown = sorted(
        path
        for path in ROOT.rglob("*.md")
        if not any(part in {"target", "dist", ".git"} for part in path.relative_to(ROOT).parts)
    )
    for path in markdown:
        check_links(path, read(path))
    check_adrs()
    check_plans()
    check_templates()
    check_summary()
    check_budget()
    if errors:
        print("\n".join(errors))
        return 1
    root_count = len(list((ROOT / "docs/src").glob("*.md")))
    print(
        f"docs-check: ok ({len(markdown)} Markdown files; "
        f"root={root_count}/{ROOT_DOC_LIMIT})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
