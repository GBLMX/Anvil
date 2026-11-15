#!/usr/bin/env python3
"""Measure large-workspace startup, search, rejection, and tree gates."""

from __future__ import annotations

import argparse
import fcntl
import json
import os
import pty
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path

ROWS = 40
COLS = 120
FILE_COUNT = 100_000
TREE_DIRS = 256
GB = 1024 * 1024 * 1024


def spawn(binary: Path, workspace: Path, target: Path | None, state: Path):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    env = dict(os.environ)
    env.update(
        XDG_CONFIG_HOME=str(state / "config"),
        XDG_RUNTIME_DIR=str(state / "run"),
        XDG_STATE_HOME=str(state / "state"),
        ANVIL_SKIP_TERMINAL_QUERY="1",
        TERM="xterm-256color",
    )
    args = [str(binary), "--workspace", str(workspace)]
    if target is not None:
        args.append(str(target))
    process = subprocess.Popen(
        args,
        cwd=workspace,
        env=env,
        stdin=slave,
        stdout=slave,
        stderr=slave,
        start_new_session=True,
    )
    os.close(slave)
    return process, master


def read_until(master: int, needle: bytes, timeout: float) -> tuple[float | None, bytes]:
    start = time.monotonic()
    output = b""
    while time.monotonic() - start < timeout:
        ready, _, _ = select.select([master], [], [], 0.05)
        if not ready:
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError:
            break
        if not chunk:
            break
        output += chunk
        if needle in output:
            return time.monotonic() - start, output
    return None, output


def one_run(
    binary: Path,
    workspace: Path,
    target: Path | None,
    start_needle: bytes,
    actions: list[tuple[bytes, bytes]],
    timeout: float,
) -> list[float]:
    state = Path(tempfile.mkdtemp(prefix="anvil-perf-state."))
    (state / "run").mkdir(parents=True)
    process, master = spawn(binary, workspace, target, state)
    times = []
    try:
        elapsed, output = read_until(master, start_needle, timeout)
        if elapsed is None:
            raise RuntimeError(
                f"startup marker {start_needle!r} missing; tail={output[-400:]!r}"
            )
        times.append(elapsed)
        for send, needle in actions:
            os.write(master, send)
            elapsed, output = read_until(master, needle, timeout)
            if elapsed is None:
                raise RuntimeError(
                    f"action marker {needle!r} missing; tail={output[-400:]!r}"
                )
            times.append(elapsed)
        return times
    finally:
        try:
            os.write(master, b":q!\r")
        except OSError:
            pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        try:
            os.close(master)
        except OSError:
            pass
        shutil.rmtree(state, ignore_errors=True)


def summarize(samples: list[float]) -> dict[str, float]:
    ordered = sorted(samples)
    return {
        "p50_ms": ordered[len(ordered) // 2] * 1000,
        "max_ms": ordered[-1] * 1000,
        "runs": float(len(ordered)),
    }


def make_workspace(root: Path) -> dict[str, Path]:
    big = root / "big"
    big.mkdir()
    for directory in range(100):
        child = big / f"d{directory:03}"
        child.mkdir()
        for file in range(1000):
            (child / f"f{file:04}.txt").write_text("filler\n")
    (big / "needle.txt").write_text("unique_needle_marker\n")

    tree = root / "tree"
    tree.mkdir()
    for directory in range(TREE_DIRS):
        (tree / f"dir{directory:03}").mkdir()
    (tree / "main.txt").write_text("tree\n")

    large = root / "one-gb.log"
    line = b"00000000 log line filler data\n"
    block = line * 200_000
    with large.open("wb") as handle:
        remaining = GB
        while remaining > 0:
            piece = block[: min(remaining, len(block))]
            handle.write(piece)
            remaining -= len(piece)
    return {"big": big, "tree": tree, "large": large}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/anvil"))
    parser.add_argument("--iterations", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"binary not found: {binary}")

    root = Path(tempfile.mkdtemp(prefix="anvil-perf-gates."))
    try:
        fixtures = make_workspace(root)
        scenarios: dict[str, list[float]] = {}
        for _ in range(args.iterations):
            scenarios.setdefault("startup_100k_ms", []).append(
                one_run(
                    binary,
                    fixtures["big"],
                    fixtures["big"] / "needle.txt",
                    b"needle.txt",
                    [],
                    args.timeout,
                )[0]
            )
            scenarios.setdefault("search_100k_ms", []).append(
                one_run(
                    binary,
                    fixtures["big"],
                    fixtures["big"] / "needle.txt",
                    b"needle.txt",
                    [(b":search-project unique_needle_marker\r", b"Search: unique_needle_marker")],
                    args.timeout,
                )[1]
            )
            scenarios.setdefault("tree_256_ms", []).append(
                one_run(
                    binary,
                    fixtures["tree"],
                    fixtures["tree"] / "main.txt",
                    b"main.txt",
                    [(b" e", b"dir000")],
                    args.timeout,
                )[1]
            )
            try:
                scenarios.setdefault("reject_1gb_ms", []).append(
                    one_run(
                        binary,
                        root,
                        fixtures["large"],
                        b"exceeds",
                        [],
                        args.timeout,
                    )[0]
                )
            except RuntimeError as error:
                print(f"1 GB rejection failed: {error}", file=sys.stderr)
                return 1

        gates = {
            "startup_100k_ms": 100.0,
            "search_100k_ms": 2000.0,
            "tree_256_ms": 1000.0,
            "reject_1gb_ms": 2000.0,
        }
        result = {}
        failed = False
        for name, samples in scenarios.items():
            summary = summarize(samples)
            summary["gate_ms"] = gates[name]
            summary["ok"] = summary["p50_ms"] <= gates[name]
            result[name] = summary
            failed |= not summary["ok"]
        print(json.dumps(result, indent=2, sort_keys=True))
        return 1 if failed else 0
    finally:
        if args.keep:
            print(f"fixtures kept at {root}", file=sys.stderr)
        else:
            shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
