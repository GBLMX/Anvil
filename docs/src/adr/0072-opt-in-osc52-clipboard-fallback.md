# ADR-0072: Opt-In OSC 52 Clipboard Write Fallback

- Status: Accepted; implemented 2026-11-23
- Version: 1.0.0
- Date: 2026-11-23
- Related: [ADR-0013](0013-kernel-distribution-boundary.md),
  [ADR-0071](0071-tui-terminal-capability-and-frame-delivery.md),
  [Threat model](../threat-model.md)
- Supersedes:
- Superseded by:

## Context

`anvil-platform` currently copies through `wl-copy` on Wayland and `xclip` on
X11. Remote SSH sessions and minimal systems often have neither program, even
when the terminal emulator supports clipboard integration. The result is a
visible failure when a user yanks to the system clipboard register.

OSC 52 lets a terminal emulator own the clipboard write. It is a terminal
escape sequence, not a filesystem or compositor API. It is also a security
boundary: a process that can write to the terminal can write to the user's
clipboard, including over SSH.

## Decision

1. OSC 52 is a **write-only fallback** for `SystemClipboard::set_text`.
2. It is disabled by default. `ANVIL_OSC52=1` enables it.
3. The native clipboard backend is attempted first. OSC 52 is attempted only
   when the native copy fails and the fallback is enabled.
4. Clipboard reads continue to use the native backend. OSC 52 paste is not
   implemented because it requires terminal response handling and a different
   trust model.
5. Payloads larger than 128 KiB are not sent through OSC 52.
6. The payload is base64 encoded and framed as
   `<ESC>]52;c;<base64><BEL>`. Clipboard content is not interpreted as terminal
   controls.
7. The write goes to the controlling terminal (`/dev/tty` on Unix,
   `CONOUT$` on Windows), never to a redirected stdout.
8. Runtime plugins still cannot emit terminal escapes or request clipboard
   writes. Only user-initiated yank/register paths reach this backend.

## Consequences

Positive:

- Clipboard yanks work over SSH and on minimal systems without `wl-copy` or
  `xclip`, if the terminal supports OSC 52.
- The safer native backend remains the default.
- The opt-in makes the terminal-boundary change explicit.

Negative:

- OSC 52 support and clipboard policy vary by terminal and multiplexer.
- Clipboard reads still require native tools.
- A user who enables OSC 52 accepts that the terminal emulator handles the
  clipboard payload.

## Verification

- `osc52_sequence("hello")` is
  `<ESC>]52;c;aGVsbG8=<BEL>`.
- The fallback is disabled with no override, `ANVIL_OSC52=0`, and enabled with
  `1` or `true`.
- Native copy remains first; OSC 52 is used only after a native failure.
- Oversized payloads are rejected by the fallback.
- Clipboard writes use `/dev/tty`/`CONOUT$`, not arbitrary stdout.
- The existing clipboard unit tests and workspace checks pass.

## References

- [XTerm Control Sequences: Operating System Commands](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html):
  OSC 52 clipboard selection and base64 payload form.
- [Terminal clipboard support discussion](https://github.com/kovidgoyal/kitty/blob/master/docs/src/clipboard.rst):
  terminal-side clipboard behavior and read/write limitations.
