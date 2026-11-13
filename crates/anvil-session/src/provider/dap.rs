use super::{
    ProviderError, ProviderId, ProviderOrigin, ProviderPriority, ProviderRegistry, RegistryError,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Borrowed context for selecting a debug adapter.
pub struct DapContext<'a> {
    pub path: Option<&'a Path>,
    pub language_id: &'a str,
}

/// One adapter command proposed by a provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DapAdapterSpec {
    pub id: ProviderId,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub language_id: String,
    pub cwd: Option<PathBuf>,
}

/// A provider of debug adapter commands.
pub trait DapAdapterProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    fn adapter_for(&self, context: &DapContext<'_>) -> Option<DapAdapterSpec>;
}

/// Validates provider output before a direct adapter spawn.
pub struct DapAdapterManager {
    providers: ProviderRegistry<dyn DapAdapterProvider>,
}

impl Default for DapAdapterManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DapAdapterManager {
    pub fn new() -> Self {
        Self {
            providers: ProviderRegistry::new(),
        }
    }

    /// Registers a provider. Runtime plugins may not spawn debug adapters.
    pub fn register(
        &mut self,
        id: ProviderId,
        priority: ProviderPriority,
        origin: ProviderOrigin,
        provider: Arc<dyn DapAdapterProvider>,
    ) -> Result<(), RegistryError> {
        if matches!(origin, ProviderOrigin::RuntimePlugin(_)) {
            return Err(RegistryError::UnsupportedOrigin(origin));
        }
        self.providers.register(id, priority, origin, provider)
    }

    pub fn unregister(&mut self, id: &ProviderId) -> bool {
        self.providers.unregister(id)
    }

    /// Resolves the first provider spec whose program and cwd pass validation.
    pub fn resolve(
        &self,
        context: &DapContext<'_>,
        workspace: &Path,
    ) -> Result<Option<DapAdapterSpec>, ProviderError> {
        let Some(spec) = self
            .providers
            .providers()
            .into_iter()
            .filter(|entry| !entry.token.is_cancelled())
            .filter_map(|entry| entry.provider.adapter_for(context))
            .next()
        else {
            return Ok(None);
        };
        if spec.language_id != context.language_id {
            return Err(ProviderError::InvalidData(
                "adapter language does not match request".to_owned(),
            ));
        }
        let program = spec.program.canonicalize().map_err(|_| {
            ProviderError::InvalidData("adapter program is not executable".to_owned())
        })?;
        if !program.is_file() {
            return Err(ProviderError::InvalidData(
                "adapter program is not a file".to_owned(),
            ));
        }
        let root = workspace
            .canonicalize()
            .map_err(|_| ProviderError::InvalidData("workspace is unavailable".to_owned()))?;
        let cwd = match spec.cwd.as_deref() {
            Some(cwd) => cwd
                .canonicalize()
                .map_err(|_| ProviderError::InvalidData("adapter cwd is unavailable".to_owned()))?,
            None => root.clone(),
        };
        let relative = cwd.strip_prefix(&root).map_err(|_| {
            ProviderError::InvalidData("adapter cwd escapes the workspace".to_owned())
        })?;
        if relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(ProviderError::InvalidData(
                "adapter cwd escapes the workspace".to_owned(),
            ));
        }
        Ok(Some(DapAdapterSpec {
            program,
            cwd: Some(cwd),
            ..spec
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderOrigin;

    struct FakeProvider {
        spec: DapAdapterSpec,
    }

    impl DapAdapterProvider for FakeProvider {
        fn id(&self) -> ProviderId {
            self.spec.id.clone()
        }

        fn adapter_for(&self, _context: &DapContext<'_>) -> Option<DapAdapterSpec> {
            Some(self.spec.clone())
        }
    }

    fn manager(spec: DapAdapterSpec, origin: ProviderOrigin) -> DapAdapterManager {
        let mut manager = DapAdapterManager::new();
        manager
            .register(
                spec.id.clone(),
                ProviderPriority::new(1),
                origin,
                Arc::new(FakeProvider { spec }),
            )
            .expect("register");
        manager
    }

    fn spec(workspace: &Path) -> DapAdapterSpec {
        DapAdapterSpec {
            id: ProviderId::new("builtin.dap"),
            program: PathBuf::from("/bin/sh"),
            args: Vec::new(),
            language_id: "rust".to_owned(),
            cwd: Some(workspace.to_path_buf()),
        }
    }

    #[test]
    fn builtin_adapter_is_resolved() {
        let workspace = std::env::temp_dir();
        let manager = manager(spec(&workspace), ProviderOrigin::Builtin);
        let resolved = manager
            .resolve(
                &DapContext {
                    path: None,
                    language_id: "rust",
                },
                &workspace,
            )
            .expect("resolve")
            .expect("adapter");
        assert!(resolved.program.is_file());
    }

    #[test]
    fn plugin_adapter_is_rejected_at_registration() {
        let mut manager = DapAdapterManager::new();
        let spec = spec(&std::env::temp_dir());
        let error = manager
            .register(
                spec.id.clone(),
                ProviderPriority::new(1),
                ProviderOrigin::RuntimePlugin("evil".into()),
                Arc::new(FakeProvider { spec }),
            )
            .expect_err("plugin adapter");
        assert!(matches!(error, RegistryError::UnsupportedOrigin(_)));
    }

    #[test]
    fn cwd_escape_is_rejected() {
        let workspace = std::env::temp_dir().join("anvil-dap-workspace");
        let outside = std::env::temp_dir().join("anvil-dap-outside");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::create_dir_all(&outside).expect("outside");
        let mut spec = spec(&workspace);
        spec.cwd = Some(outside);
        let manager = manager(spec, ProviderOrigin::Builtin);
        assert_eq!(
            manager.resolve(
                &DapContext {
                    path: None,
                    language_id: "rust",
                },
                &workspace,
            ),
            Err(ProviderError::InvalidData(
                "adapter cwd escapes the workspace".to_owned()
            ))
        );
    }
}
