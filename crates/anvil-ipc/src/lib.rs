//! Framing for Anvil's length-prefixed JSON-RPC transport.
//!
//! Each frame is a 4-byte big-endian length followed by that many bytes of
//! UTF-8 JSON-RPC. The codec is independent of the socket type so it can be
//! tested with an in-memory reader and reused by the editor and plugin host.

use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::Duration,
};

/// Maximum accepted frame body.
pub const MAX_FRAME_LEN: u32 = 8 * 1024 * 1024;

/// A framing failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// The peer closed before sending a complete header.
    TruncatedHeader,
    /// The peer closed after the header but before the body ended.
    TruncatedBody,
    /// The header declared a body larger than [`MAX_FRAME_LEN`].
    TooLarge(u32),
    /// The underlying stream failed.
    Io(io::ErrorKind),
    /// A zero-length body is not a valid JSON-RPC frame.
    Empty,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TruncatedHeader => write!(formatter, "truncated frame header"),
            Self::TruncatedBody => write!(formatter, "truncated frame body"),
            Self::Io(kind) => write!(formatter, "frame I/O failed: {kind:?}"),
            Self::TooLarge(length) => write!(formatter, "frame length {length} exceeds maximum"),
            Self::Empty => write!(formatter, "empty frame"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Read one frame.
///
/// Returns `Ok(None)` only on a clean EOF before the first header byte.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>, FrameError> {
    let mut header = [0u8; 4];
    let mut read = 0;
    while read < header.len() {
        match reader.read(&mut header[read..]) {
            Ok(0) if read == 0 => return Ok(None),
            Ok(0) => return Err(FrameError::TruncatedHeader),
            Ok(count) => read += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(io_error(error)),
        }
    }

    let length = u32::from_be_bytes(header);
    if length == 0 {
        return Err(FrameError::Empty);
    }
    if length > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(length));
    }

    let mut body = vec![0u8; length as usize];
    let mut read = 0;
    while read < body.len() {
        match reader.read(&mut body[read..]) {
            Ok(0) => return Err(FrameError::TruncatedBody),
            Ok(count) => read += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(Some(body))
}

/// Write one frame.
pub fn write_frame<W: Write>(writer: &mut W, body: &[u8]) -> Result<(), FrameError> {
    if body.is_empty() {
        return Err(FrameError::Empty);
    }
    let length = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(u32::MAX))?;
    if length > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(length));
    }
    writer.write_all(&length.to_be_bytes()).map_err(io_error)?;
    writer.write_all(body).map_err(io_error)?;
    writer.flush().map_err(io_error)
}

fn io_error(error: io::Error) -> FrameError {
    FrameError::Io(error.kind())
}

/// A Unix socket listener for framed connections.
#[derive(Debug)]
pub struct Listener {
    inner: UnixListener,
    path: PathBuf,
}

impl Listener {
    /// Bind a Unix socket and set its mode to `0600`.
    ///
    /// The caller owns the parent directory and the socket path. Binding an
    /// existing path is not silently repaired; a failed permission change
    /// removes the socket this call just created.
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let inner = UnixListener::bind(&path)?;
        if let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
            drop(inner);
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        Ok(Self { inner, path })
    }

    /// Accept one framed connection.
    pub fn accept(&self) -> io::Result<Connection> {
        let (stream, _) = self.inner.accept()?;
        Ok(Connection { stream })
    }

    /// Path the listener is bound to.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Linux peer credentials from `SO_PEERCRED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    pub pid: i32,
    pub uid: u32,
    pub gid: u32,
}

/// A framed connection over a Unix stream.
#[derive(Debug)]
pub struct Connection {
    stream: UnixStream,
}

impl Connection {
    /// Connect to a Unix socket.
    pub fn connect(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            stream: UnixStream::connect(path)?,
        })
    }

    /// Read the Linux peer credentials for this connection.
    pub fn peer_credentials(&self) -> io::Result<PeerCredentials> {
        let credentials = rustix::net::sockopt::socket_peercred(&self.stream)?;
        Ok(PeerCredentials {
            pid: credentials.pid.as_raw_pid(),
            uid: credentials.uid.as_raw(),
            gid: credentials.gid.as_raw(),
        })
    }

    /// Send one frame.
    pub fn send(&mut self, body: &[u8]) -> Result<(), FrameError> {
        write_frame(&mut self.stream, body)
    }

    /// Receive one frame, or `None` on a clean peer EOF.
    pub fn recv(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        read_frame(&mut self.stream)
    }

    /// Set the read timeout for subsequent `recv` calls.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Cursor, os::unix::fs::PermissionsExt, path::PathBuf};

    #[test]
    fn round_trips_a_frame() {
        let mut encoded = Vec::new();
        write_frame(&mut encoded, br#"{"jsonrpc":"2.0"}"#).expect("write");
        assert_eq!(&encoded[..4], &[0, 0, 0, 17]);
        let mut cursor = Cursor::new(encoded);
        let frame = read_frame(&mut cursor).expect("read").expect("frame");
        assert_eq!(frame, br#"{"jsonrpc":"2.0"}"#);
        assert!(read_frame(&mut cursor).expect("eof").is_none());
    }

    #[test]
    fn rejects_empty_frames() {
        let mut encoded = Vec::new();
        assert_eq!(write_frame(&mut encoded, b""), Err(FrameError::Empty));

        let mut cursor = Cursor::new(vec![0, 0, 0, 0]);
        assert_eq!(read_frame(&mut cursor), Err(FrameError::Empty));
    }

    #[test]
    fn rejects_truncated_header_and_body() {
        let mut header = Cursor::new(vec![0, 0]);
        assert_eq!(read_frame(&mut header), Err(FrameError::TruncatedHeader));

        let mut body = Cursor::new(vec![0, 0, 0, 4, b'{']);
        assert_eq!(read_frame(&mut body), Err(FrameError::TruncatedBody));
    }

    #[test]
    fn rejects_frames_over_the_limit() {
        let oversized = MAX_FRAME_LEN + 1;
        let mut encoded = Vec::new();
        assert_eq!(
            write_frame(&mut encoded, &vec![b' '; oversized as usize]),
            Err(FrameError::TooLarge(oversized))
        );

        let mut header = Cursor::new(oversized.to_be_bytes().to_vec());
        assert_eq!(
            read_frame(&mut header),
            Err(FrameError::TooLarge(oversized))
        );
    }

    #[test]
    fn preserves_io_error_kind() {
        struct Failing;

        impl Read for Failing {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset"))
            }
        }

        assert_eq!(
            read_frame(&mut Failing),
            Err(FrameError::Io(io::ErrorKind::ConnectionReset))
        );
    }

    fn socket_path() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "anvil-ipc-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    struct SocketCleanup(PathBuf);

    impl Drop for SocketCleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn unix_socket_round_trip_and_clean_eof() {
        let path = socket_path();
        let _cleanup = SocketCleanup(path.clone());
        let listener = Listener::bind(&path).expect("bind");
        assert_eq!(
            fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
            0o600
        );

        let mut client = Connection::connect(&path).expect("connect");
        let mut server = listener.accept().expect("accept");
        assert_eq!(
            server.peer_credentials().expect("peer credentials").pid,
            std::process::id() as i32
        );
        client.send(b"hello").expect("send");
        assert_eq!(server.recv().expect("recv"), Some(b"hello".to_vec()));
        server.send(b"world").expect("send");
        assert_eq!(client.recv().expect("recv"), Some(b"world".to_vec()));

        drop(client);
        assert_eq!(server.recv().expect("eof"), None);
    }
}
