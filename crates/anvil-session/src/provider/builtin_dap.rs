use super::{DapAdapterProvider, DapAdapterSpec, DapContext, ProviderId};
use std::path::PathBuf;

/// Built-in Rust adapter. The adapter is resolved to an absolute executable.
pub struct RustDapProvider;

impl DapAdapterProvider for RustDapProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("builtin.lldb-dap")
    }

    fn adapter_for(&self, context: &DapContext<'_>) -> Option<DapAdapterSpec> {
        if context.language_id != "rust" {
            return None;
        }
        let program = ["/usr/bin/lldb-dap", "/usr/local/bin/lldb-dap"]
            .into_iter()
            .map(PathBuf::from)
            .find(|candidate| candidate.is_file())?;
        Some(DapAdapterSpec {
            id: self.id(),
            program,
            args: Vec::new(),
            language_id: "rust".to_owned(),
            cwd: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{DapAdapterManager, ProviderOrigin, ProviderPriority};
    use std::path::Path;
    use std::sync::Arc;

    #[test]
    fn rust_provider_is_accepted_and_resolves() {
        let provider = RustDapProvider;
        let context = DapContext {
            path: Some(Path::new("src/main.rs")),
            language_id: "rust",
        };
        let Some(spec) = provider.adapter_for(&context) else {
            return;
        };
        let workspace = std::env::temp_dir();
        let mut manager = DapAdapterManager::new();
        manager
            .register(
                provider.id(),
                ProviderPriority::new(1),
                ProviderOrigin::Builtin,
                Arc::new(provider),
            )
            .expect("register");
        let resolved = manager
            .resolve(&context, &workspace)
            .expect("resolve")
            .expect("adapter");
        assert_eq!(resolved.program, spec.program.canonicalize().unwrap());
        assert_eq!(resolved.language_id, "rust");
    }
}
