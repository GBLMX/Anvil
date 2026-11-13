use std::fmt;

/// Why a provider request or provider operation failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderError {
    Unavailable,
    Cancelled,
    Timeout,
    InvalidData(String),
    Internal(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("provider unavailable"),
            Self::Cancelled => formatter.write_str("provider request cancelled"),
            Self::Timeout => formatter.write_str("provider request timed out"),
            Self::InvalidData(message) => write!(formatter, "invalid provider data: {message}"),
            Self::Internal(message) => write!(formatter, "provider failure: {message}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Why a provider could not be registered or removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    InvalidId(crate::provider::ProviderId),
    DuplicateId(crate::provider::ProviderId),
    UnsupportedOrigin(crate::provider::ProviderOrigin),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(id) => write!(formatter, "invalid provider id: {id}"),
            Self::DuplicateId(id) => write!(formatter, "duplicate provider id: {id}"),
            Self::UnsupportedOrigin(origin) => {
                write!(formatter, "provider origin is not supported: {origin:?}")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_states_are_distinct() {
        assert_ne!(ProviderError::Unavailable, ProviderError::Cancelled);
        assert_ne!(ProviderError::Timeout, ProviderError::Unavailable);
        assert_ne!(
            ProviderError::InvalidData("bad".to_owned()),
            ProviderError::Internal("bad".to_owned())
        );
    }
}
