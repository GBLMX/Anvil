//! Terminal background detection for automatic light/dark theme selection.

#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
const QUERY: &[u8] = b"\x1b]11;?\x07";
#[cfg(unix)]
const MAX_RESPONSE: usize = 128;
#[cfg(unix)]
const QUERY_TIMEOUT: Duration = Duration::from_millis(200);
#[cfg(unix)]
const DRAIN_TIMEOUT: Duration = Duration::from_millis(100);

/// Returns whether the terminal background is light.
///
/// OSC 11 is only read when terminal queries are enabled. A missing or
/// unparseable response returns `None` so the caller can fall back to
/// `COLORFGBG` or the dark default.
pub(crate) fn query_is_light() -> Option<bool> {
    if std::env::var_os("ANVIL_SKIP_TERMINAL_QUERY").is_some() {
        return None;
    }
    #[cfg(unix)]
    {
        crossterm::terminal::enable_raw_mode().ok()?;
        let response = read_response();
        let _ = crossterm::terminal::disable_raw_mode();
        parse_is_light(&response?)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(unix)]
fn read_response() -> Option<Vec<u8>> {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, BorrowedFd};

    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    tty.write_all(QUERY).ok()?;
    tty.flush().ok()?;

    let fd = tty.as_raw_fd();
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    let mut poll_fds = [PollFd::new(borrowed, PollFlags::POLLIN)];
    let deadline = Instant::now() + QUERY_TIMEOUT;
    let drain_deadline = deadline + DRAIN_TIMEOUT;
    let mut response = Vec::with_capacity(32);
    while Instant::now() < drain_deadline {
        let timeout: u16 = if Instant::now() < deadline { 15 } else { 10 };
        match poll(&mut poll_fds, PollTimeout::from(timeout)) {
            Ok(0) => {}
            Ok(_) => {
                let mut byte = [0; 1];
                if tty.read(&mut byte).ok()? == 0 {
                    break;
                }
                response.push(byte[0]);
                if response.ends_with(b"\x07") || response.ends_with(b"\x1b\\") {
                    return Some(response);
                }
                if response.len() >= MAX_RESPONSE {
                    break;
                }
            }
            Err(nix::errno::Errno::EINTR) => {}
            Err(_) => break,
        }
    }
    None
}

#[cfg(any(unix, test))]
fn parse_is_light(response: &[u8]) -> Option<bool> {
    let text = String::from_utf8_lossy(response);
    let start = text.find("]11;rgb:")? + "]11;rgb:".len();
    let rest = &text[start..];
    let end = rest.find(['\x07', '\x1b']).unwrap_or(rest.len());
    let mut channels = rest[..end].split('/');
    let red = parse_channel(channels.next()?)?;
    let green = parse_channel(channels.next()?)?;
    let blue = parse_channel(channels.next()?)?;
    Some(relative_luminance(red, green, blue) > 0.5)
}

#[cfg(any(unix, test))]
fn parse_channel(text: &str) -> Option<u8> {
    if text.is_empty() || text.len() > 4 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u16::from_str_radix(text, 16).ok()?;
    Some(match text.len() {
        1 | 2 => value as u8,
        3 => (value >> 4) as u8,
        _ => (value >> 8) as u8,
    })
}

#[cfg(any(unix, test))]
fn relative_luminance(red: u8, green: u8, blue: u8) -> f64 {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dark_and_light_osc_responses() {
        assert_eq!(
            parse_is_light(b"\x1b]11;rgb:1e1e/1e1e/2e2e\x07"),
            Some(false)
        );
        assert_eq!(
            parse_is_light(b"\x1b]11;rgb:fdf6/e3e3/d5d5\x1b\\"),
            Some(true)
        );
    }

    #[test]
    fn rejects_malformed_responses() {
        assert_eq!(parse_is_light(b""), None);
        assert_eq!(parse_is_light(b"\x1b]11;rgb:zz/00/00\x07"), None);
        assert_eq!(parse_is_light(b"\x1b]11;rgb:ffff/0000\x07"), None);
    }
}
