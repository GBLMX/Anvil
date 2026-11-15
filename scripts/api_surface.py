#!/usr/bin/env python3
"""Hash public Rust declarations, including signatures, fields, and variants."""

from __future__ import annotations

import hashlib
import pathlib
import sys


def strip_comments(text: str) -> str:
    out: list[str] = []
    index = 0
    length = len(text)
    while index < length:
        if text.startswith("//", index):
            newline = text.find("\n", index)
            index = length if newline == -1 else newline
            continue
        if text.startswith("/*", index):
            depth = 1
            index += 2
            while index < length and depth:
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            continue
        out.append(text[index])
        index += 1
    return "".join(out)


def declaration_end(text: str, start: int, kind: str) -> int:
    index = start
    length = len(text)
    parens = 0
    brackets = 0
    braces = 0
    if kind == "fn":
        while index < length:
            char = text[index]
            if char == "(":
                parens += 1
            elif char == ")":
                parens = max(0, parens - 1)
            elif char == "[":
                brackets += 1
            elif char == "]":
                brackets = max(0, brackets - 1)
            elif char == ";":
                return index + 1
            elif char == "{" and parens == 0 and brackets == 0:
                return index
            index += 1
        return length
    while index < length:
        char = text[index]
        if char == "{":
            braces += 1
        elif char == "}":
            braces = max(0, braces - 1)
            if braces == 0 and kind in {"struct", "enum", "union", "trait"}:
                return index + 1
        elif char == ";" and braces == 0:
            return index + 1
        index += 1
    return length


def declarations(text: str) -> list[str]:
    text = strip_comments(text)
    items: list[str] = []
    index = 0
    length = len(text)
    while True:
        index = text.find("pub", index)
        if index == -1:
            break
        before = text[index - 1] if index else " "
        after = text[index + 3] if index + 3 < length else " "
        if (before.isalnum() or before == "_") or (after.isalnum() or after == "_"):
            index += 3
            continue
        cursor = index + 3
        while cursor < length and text[cursor].isspace():
            cursor += 1
        if cursor < length and text[cursor] == "(":
            index += 3
            continue
        kind = ""
        while cursor < length:
            while cursor < length and text[cursor].isspace():
                cursor += 1
            word_start = cursor
            while cursor < length and (text[cursor].isalnum() or text[cursor] == "_"):
                cursor += 1
            word = text[word_start:cursor]
            if word in {"fn", "struct", "enum", "trait", "union", "type", "const", "static", "mod", "use"}:
                kind = "fn" if word == "const" and text[cursor:].lstrip().startswith("fn") else word
                if word == "const":
                    remainder = text[cursor:].lstrip()
                    if remainder.startswith("fn"):
                        kind = "fn"
                break
            if word in {"async", "unsafe", "extern", "default"}:
                continue
            if not word:
                break
            kind = ""
            break
        if not kind:
            index += 3
            continue
        end = declaration_end(text, index, kind)
        item = " ".join(text[index:end].split())
        if item:
            items.append(item)
        index = max(end, index + 3)
    return items


def main() -> int:
    paths = [pathlib.Path(arg) for arg in sys.argv[1:]]
    entries: list[str] = []
    for path in paths:
        for file in sorted(path.rglob("*.rs")):
            entries.extend(f"{file}:{item}" for item in declarations(file.read_text()))
    entries.sort()
    digest = hashlib.sha256("\n".join(entries).encode()).hexdigest()
    print(digest)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
