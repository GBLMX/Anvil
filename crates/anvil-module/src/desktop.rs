//! Desktop handoff extension-point types.

/// Failure while handing a URL to the desktop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopError {
    InvalidUrl,
    Unavailable(String),
    Spawn(String),
}

impl std::fmt::Display for DesktopError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl => formatter.write_str("invalid URL"),
            Self::Unavailable(message) | Self::Spawn(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for DesktopError {}

/// UI-neutral URL handoff service.
pub trait DesktopService: Send + Sync {
    fn open_url(&self, url: &str) -> Result<(), DesktopError>;
}
