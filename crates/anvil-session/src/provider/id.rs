use anvil_api::ModuleId;
use std::sync::Arc;

/// Stable provider identity in `<vendor>.<name>` form.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(Arc<str>);

impl ProviderId {
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_valid(&self) -> bool {
        let Some((vendor, name)) = self.0.split_once('.') else {
            return false;
        };
        !vendor.is_empty()
            && !name.is_empty()
            && !name.contains('.')
            && vendor.bytes().chain(name.bytes()).all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Where a provider came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderOrigin {
    Builtin,
    RustModule(ModuleId),
    RuntimePlugin(Arc<str>),
}

/// Ordering weight for providers from the same capability. Higher values run
/// first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProviderPriority(i32);

impl ProviderPriority {
    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_provider_ids() {
        assert!(ProviderId::new("builtin.buffer").is_valid());
        assert!(ProviderId::new("official.git-status").is_valid());
        assert!(!ProviderId::new("buffer").is_valid());
        assert!(!ProviderId::new("vendor.name.extra").is_valid());
        assert!(!ProviderId::new("Vendor.Name").is_valid());
    }
}
