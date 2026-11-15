#!/usr/bin/env python3
"""Measure large-workspace startup, search, tree, watch, and size gates."""

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
MB = 1024 * 1024


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
    large_200 = root / "200mb.log"
    with large_200.open("wb") as handle:
        remaining = 200 * MB
        while remaining > 0:
            piece = block[: min(remaining, len(block))]
            handle.write(piece)
            remaining -= len(piece)
    watch = root / "watch"
    watch.mkdir()
    (watch / "main.txt").write_text("initial\n")
    return {
        "big": big,
        "tree": tree,
        "large": large,
        "large_200": large_200,
        "watch": watch,
    }


def polling_watch_latency(binary: Path, workspace: Path, timeout: float) -> float:
    target = workspace / "main.txt"
    state = Path(tempfile.mkdtemp(prefix="anvil-perf-watch."))
    (state / "run").mkdir(parents=True)
    process, master = spawn(binary, workspace, target, state)
    try:
        elapsed, output = read_until(master, b"main.txt", timeout)
        if elapsed is None:
            raise RuntimeError(f"watch startup missing; tail={output[-400:]!r}")
        start = time.monotonic()
        target.write_text("changed on disk\n")
        elapsed, output = read_until(master, b"reloaded", 5.0)
        if elapsed is None:
            raise RuntimeError(f"polling did not reload; tail={output[-400:]!r}")
        return time.monotonic() - start
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


def drain(master: int) -> None:
    while select.select([master], [], [], 0.05)[0]:
        try:
            if not os.read(master, 65536):
                break
        except OSError:
            break

def large_file_latency(
    binary: Path, target: Path, timeout: float, scroll_samples: int = 10
) -> tuple[float, list[float]]:
    state = Path(tempfile.mkdtemp(prefix="anvil-perf-large."))
    (state / "run").mkdir(parents=True)
    process, master = spawn(binary, target.parent, target, state)
    try:
        elapsed, output = read_until(master, b"00000000 log line filler data", timeout)
        if elapsed is None:
            raise RuntimeError(f"large-file open missing; tail={output[-400:]!r}")
        open_elapsed = elapsed
        drain(master)
        scrolls = []
        for _ in range(scroll_samples):
            os.write(master, b"j")
            elapsed, output = read_until(master, b"\x1b[", 2.0)
            if elapsed is None:
                raise RuntimeError(f"large-file scroll missing; tail={output[-400:]!r}")
            scrolls.append(elapsed)
            drain(master)
        return open_elapsed, scrolls
    finally:
        try:
            os.write(master, b":q!\r")
        except OSError:
            pass
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        try:
            os.close(master)
        except OSError:
            pass
        shutil.rmtree(state, ignore_errors=True)

def inotify_kernel_latency(path: Path) -> float | None:
    try:
        import ctypes
        import select

        libc = ctypes.CDLL("libc.so.6", use_errno=True)
        libc.inotify_init1.restype = ctypes.c_int
        fd = libc.inotify_init1(os.O_NONBLOCK | os.O_CLOEXEC)
        if fd < 0:
            return None
        try:
            watch = libc.inotify_add_watch(
                fd, os.fsencode(path), 0x00000002 | 0x00000008
            )
            if watch < 0:
                return None
            start = time.monotonic()
            path.write_text("kernel event\n")
            if not select.select([fd], [], [], 5.0)[0]:
                return None
            os.read(fd, 4096)
            return time.monotonic() - start
        finally:
            os.close(fd)
    except OSError:
        return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/anvil"))
    parser.add_argument("--gateway", type=Path)
    parser.add_argument("--iterations", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"binary not found: {binary}")
    gateway = args.gateway
    if gateway is not None:
        gateway = gateway.resolve()
        if not gateway.is_file():
            parser.error(f"gateway not found: {gateway}")
    else:
        repository = Path(__file__).resolve().parent.parent
        candidates = [
            os.environ.get("ANVIL_GATEWAY"),
            str(binary.parent / "anvil-gateway"),
            str(repository / "dist/anvil/bin/anvil-gateway"),
        ]
        gateway = next(
            (
                Path(candidate).resolve()
                for candidate in candidates
                if candidate and Path(candidate).is_file()
            ),
            None,
        )
    if gateway is None:
        parser.error(
            "gateway not found; run scripts/release-layout.sh and use "
            "--binary <layout>/bin/anvil, or pass --gateway"
        )
    os.environ["ANVIL_GATEWAY"] = str(gateway)

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
                opened, scrolls = large_file_latency(
                    binary, fixtures["large_200"], args.timeout
                )
            except RuntimeError as error:
                print(f"200 MB open failed: {error}", file=sys.stderr)
                return 1
            scroll = summarize(scrolls)
            scenarios.setdefault("open_200mb_ms", []).append(opened)
            scenarios.setdefault("scroll_200mb_ms", []).append(scroll["p50_ms"] / 1000)
            scenarios.setdefault("scroll_200mb_max_ms", []).append(
                scroll["max_ms"] / 1000
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
            try:
                scenarios.setdefault("watch_poll_ms", []).append(
                    polling_watch_latency(binary, fixtures["watch"], args.timeout)
                )
            except RuntimeError as error:
                print(f"polling watch failed: {error}", file=sys.stderr)
                return 1
            kernel = inotify_kernel_latency(fixtures["watch"] / "main.txt")
            if kernel is not None:
                scenarios.setdefault("inotify_kernel_ms", []).append(kernel)

        gates = {
            "startup_100k_ms": 100.0,
            "search_100k_ms": 2000.0,
            "tree_256_ms": 1000.0,
            "open_200mb_ms": 2000.0,
            "scroll_200mb_ms": 33.0,
            "scroll_200mb_max_ms": 100.0,
            "reject_1gb_ms": 2000.0,
            "watch_poll_ms": 1500.0,
            "inotify_kernel_ms": 50.0,
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
