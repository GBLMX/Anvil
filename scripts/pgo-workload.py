#!/usr/bin/env python3
"""Drive Anvil through a short PTY workload for PGO/BOLT training."""

from __future__ import annotations

import argparse
import fcntl
import os
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path


def drain(fd: int, seconds: float) -> bytes:
    deadline = time.monotonic() + seconds
    data = b""
    while time.monotonic() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.05)
        if ready:
            try:
                data += os.read(fd, 65536)
            except OSError:
                break
    return data


def send(fd: int, value: bytes, wait: float = 0.15) -> None:
    os.write(fd, value)
    drain(fd, wait)


def run(binary: Path, workspace: Path, timeout: float) -> int:
    source = workspace / "main.rs"
    other = workspace / "other.txt"
    source.write_text("fn main() {\n    let value = 1;\n    println!(\"{value}\");\n}\n")
    other.write_text("alpha beta gamma\nsecond line\nthird line\n")

    master, slave = pty.openpty()
    fcntl.ioctl(
        slave,
        termios.TIOCSWINSZ,
        struct.pack("HHHH", 30, 120, 0, 0),
    )
    env = dict(os.environ)
    env["HOME"] = str(workspace)
    process = subprocess.Popen(
        [str(binary), str(source)],
        cwd=workspace,
        env=env,
        stdin=slave,
        stdout=slave,
        stderr=slave,
        start_new_session=True,
    )
    os.close(slave)
    try:
        drain(master, 1.5)
        send(master, b"i")
        send(master, b"let training = 42;\n")
        send(master, b"\x1b")
        send(master, b"\x17v")
        send(master, b"\x17l")
        send(master, b":e other.txt\r", 0.4)
        send(master, b"i")
        send(master, b"training edit\n")
        send(master, b"\x1b")
        send(master, b"\x17h")
        send(master, b"/value\n", 0.3)
        send(master, b"n")
        send(master, b"\x1b")
        send(master, b"\x17q")
        send(master, b":q!\r", 0.5)
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            print("workload did not exit after :q!", file=sys.stderr)
            return 1
        if process.returncode != 0:
            print(
                f"workload exited with {process.returncode}",
                file=sys.stderr,
            )
        return process.returncode
    finally:
        try:
            os.close(master)
        except OSError:
            pass


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("workspace", nargs="?", type=Path)
    parser.add_argument("--timeout", type=float, default=15.0)
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"binary not found: {binary}")
    workspace = args.workspace
    if workspace is None:
        workspace = Path(tempfile.mkdtemp(prefix="anvil-pgo-workload-"))
    else:
        workspace.mkdir(parents=True, exist_ok=True)
    return run(binary, workspace, args.timeout)


if __name__ == "__main__":
    raise SystemExit(main())
