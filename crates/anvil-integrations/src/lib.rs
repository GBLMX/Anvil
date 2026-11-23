//! Declarative IDE integration descriptors and per-root execution grants.
//!
//! Descriptors are metadata. [`authorize`] requires installation, enablement,
//! workspace trust, and a grant bound to the exact descriptor and executable.

use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Closed execution capabilities. Wildcards are not representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Lsp,
    Formatter,
    Dap,
    Build,
    Task,
}

impl Capability {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "exec:lsp" => Ok(Self::Lsp),
            "exec:formatter" => Ok(Self::Formatter),
            "exec:dap" => Ok(Self::Dap),
            "exec:build" => Ok(Self::Build),
            "exec:task" => Ok(Self::Task),
            _ => Err(format!("unknown capability: {value}")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lsp => "exec:lsp",
            Self::Formatter => "exec:formatter",
            Self::Dap => "exec:dap",
            Self::Build => "exec:build",
            Self::Task => "exec:task",
        }
    }

    fn applies_to(self, kind: IntegrationKind) -> bool {
        matches!(
            (self, kind),
            (Self::Lsp, IntegrationKind::Lsp)
                | (Self::Formatter, IntegrationKind::Formatter)
                | (Self::Dap, IntegrationKind::Dap)
                | (Self::Build, IntegrationKind::Build)
                | (Self::Task, IntegrationKind::Task)
        )
    }
}

/// The kind of adapter a descriptor launches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationKind {
    Lsp,
    Formatter,
    Dap,
    Build,
    Task,
}

impl IntegrationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lsp => "lsp",
            Self::Formatter => "formatter",
            Self::Dap => "dap",
            Self::Build => "build",
            Self::Task => "task",
        }
    }
}

/// One declarative tool descriptor. It carries no execution authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationDescriptor {
    pub id: String,
    pub version: String,
    pub kind: IntegrationKind,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<String>,
    pub languages: Vec<String>,
}

impl IntegrationDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 128
            || !self
                .id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
        {
            return Err("invalid integration id".to_owned());
        }
        if self.version.is_empty() || self.version.len() > 64 {
            return Err("invalid integration version".to_owned());
        }
        if !self.program.is_absolute() {
            return Err("integration program must be absolute".to_owned());
        }
        if let Some(cwd) = &self.cwd
            && (cwd.is_absolute()
                || cwd
                    .components()
                    .any(|part| matches!(part, Component::ParentDir)))
        {
            return Err("integration cwd must be workspace-relative".to_owned());
        }
        if self.env.len() > 64
            || self.env.iter().any(|name| {
                name.is_empty()
                    || name.len() > 128
                    || name.contains('=')
                    || !name
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
            })
        {
            return Err("invalid integration environment allowlist".to_owned());
        }
        if self.languages.len() > 64
            || self
                .languages
                .iter()
                .any(|language| language.is_empty() || language.len() > 64)
        {
            return Err("invalid integration languages".to_owned());
        }
        Ok(())
    }

    /// Stable descriptor hash used to bind grants.
    pub fn hash(&self) -> Result<[u8; 32], String> {
        self.validate()?;
        let mut canonical = Vec::new();
        for part in [
            self.id.as_str(),
            self.version.as_str(),
            self.kind.as_str(),
            self.program.to_string_lossy().as_ref(),
        ] {
            canonical.extend_from_slice(part.as_bytes());
            canonical.push(0);
        }
        for arg in &self.args {
            canonical.extend_from_slice(arg.as_bytes());
            canonical.push(0);
        }
        canonical.push(1);
        if let Some(cwd) = &self.cwd {
            canonical.extend_from_slice(cwd.to_string_lossy().as_bytes());
        }
        canonical.push(0);
        for name in &self.env {
            canonical.extend_from_slice(name.as_bytes());
            canonical.push(0);
        }
        canonical.push(1);
        for language in &self.languages {
            canonical.extend_from_slice(language.as_bytes());
            canonical.push(0);
        }
        Ok(Sha256::digest(canonical).into())
    }
}

/// A grant bound to one integration, root, capability, and executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub integration_id: String,
    pub version: String,
    pub descriptor_hash: [u8; 32],
    pub workspace_root: PathBuf,
    pub capability: Capability,
    pub executable: PathBuf,
}

impl Grant {
    pub fn issue(
        descriptor: &IntegrationDescriptor,
        workspace_root: impl Into<PathBuf>,
        capability: Capability,
        executable: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let workspace_root = workspace_root.into();
        let executable = executable.into();
        if !workspace_root.is_absolute() {
            return Err("workspace root must be absolute".to_owned());
        }
        if !executable.is_absolute() {
            return Err("grant executable must be absolute".to_owned());
        }
        if !capability.applies_to(descriptor.kind) {
            return Err("capability does not match integration kind".to_owned());
        }
        Ok(Self {
            integration_id: descriptor.id.clone(),
            version: descriptor.version.clone(),
            descriptor_hash: descriptor.hash()?,
            workspace_root,
            capability,
            executable,
        })
    }

    pub fn matches(
        &self,
        descriptor: &IntegrationDescriptor,
        workspace_root: &Path,
        capability: Capability,
        executable: &Path,
    ) -> bool {
        self.integration_id == descriptor.id
            && self.version == descriptor.version
            && self.capability == capability
            && self.workspace_root == workspace_root
            && self.executable == executable
            && descriptor
                .hash()
                .is_ok_and(|hash| hash == self.descriptor_hash)
    }
}

/// Per-root grant collection.
#[derive(Debug, Default, Clone)]
pub struct GrantStore {
    grants: Vec<Grant>,
}

impl GrantStore {
    pub fn grant(&mut self, grant: Grant) {
        self.grants.retain(|existing| {
            !(existing.integration_id == grant.integration_id
                && existing.workspace_root == grant.workspace_root
                && existing.capability == grant.capability)
        });
        self.grants.push(grant);
    }

    pub fn revoke(&mut self, integration_id: &str, workspace_root: &Path, capability: Capability) {
        self.grants.retain(|grant| {
            !(grant.integration_id == integration_id
                && grant.workspace_root == workspace_root
                && grant.capability == capability)
        });
    }

    pub fn find(
        &self,
        descriptor: &IntegrationDescriptor,
        workspace_root: &Path,
        capability: Capability,
        executable: &Path,
    ) -> Option<&Grant> {
        self.grants
            .iter()
            .find(|grant| grant.matches(descriptor, workspace_root, capability, executable))
    }
}

/// Inputs for one execution authorization decision.
pub struct Authorization<'a> {
    pub descriptor: &'a IntegrationDescriptor,
    pub installed: bool,
    pub enabled: bool,
    pub workspace_trusted: bool,
    pub grants: &'a GrantStore,
    pub workspace_root: &'a Path,
    pub capability: Capability,
    pub resolved_executable: &'a Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    NotInstalled,
    NotEnabled,
    UntrustedWorkspace,
    UnsupportedCapability,
    MissingGrant,
}

impl fmt::Display for Denial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotInstalled => "integration is not installed",
            Self::NotEnabled => "integration is not enabled",
            Self::UntrustedWorkspace => "workspace is not trusted",
            Self::UnsupportedCapability => "capability does not match descriptor",
            Self::MissingGrant => "specific integration grant is missing",
        })
    }
}

impl std::error::Error for Denial {}

/// Authorize one descriptor launch. Every gate must pass; a declaration grants nothing.
pub fn authorize(request: Authorization<'_>) -> Result<(), Denial> {
    if !request.installed {
        return Err(Denial::NotInstalled);
    }
    if !request.enabled {
        return Err(Denial::NotEnabled);
    }
    if !request.workspace_trusted {
        return Err(Denial::UntrustedWorkspace);
    }
    if !request.capability.applies_to(request.descriptor.kind) {
        return Err(Denial::UnsupportedCapability);
    }
    if request
        .grants
        .find(
            request.descriptor,
            request.workspace_root,
            request.capability,
            request.resolved_executable,
        )
        .is_none()
    {
        return Err(Denial::MissingGrant);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> IntegrationDescriptor {
        IntegrationDescriptor {
            id: "rust-analyzer".to_owned(),
            version: "1.0.0".to_owned(),
            kind: IntegrationKind::Lsp,
            program: PathBuf::from("/usr/bin/rust-analyzer"),
            args: vec!["--stdio".to_owned()],
            cwd: Some(PathBuf::from(".")),
            env: vec!["PATH".to_owned()],
            languages: vec!["rust".to_owned()],
        }
    }

    fn grant() -> Grant {
        Grant::issue(
            &descriptor(),
            "/work",
            Capability::Lsp,
            "/usr/bin/rust-analyzer",
        )
        .expect("grant")
    }

    fn authorization<'a>(
        descriptor: &'a IntegrationDescriptor,
        grants: &'a GrantStore,
        capability: Capability,
        executable: &'a Path,
    ) -> Authorization<'a> {
        Authorization {
            descriptor,
            installed: true,
            enabled: true,
            workspace_trusted: true,
            grants,
            workspace_root: Path::new("/work"),
            capability,
            resolved_executable: executable,
        }
    }

    #[test]
    fn capability_is_closed() {
        assert_eq!(Capability::parse("exec:lsp"), Ok(Capability::Lsp));
        assert!(Capability::parse("exec:*").is_err());
        assert!(Capability::parse("exec").is_err());
    }

    #[test]
    fn validates_descriptor_paths_and_environment() {
        let mut invalid = descriptor();
        invalid.program = PathBuf::from("rust-analyzer");
        assert!(invalid.validate().is_err());
        invalid = descriptor();
        invalid.cwd = Some(PathBuf::from("../escape"));
        assert!(invalid.validate().is_err());
        invalid = descriptor();
        invalid.env = vec!["PATH=value".to_owned()];
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn hash_changes_with_launch_metadata() {
        let original = descriptor().hash().expect("hash");
        let mut changed = descriptor();
        changed.args.push("--experimental".to_owned());
        assert_ne!(original, changed.hash().expect("hash"));
    }

    #[test]
    fn grant_requires_exact_binding() {
        let store = {
            let mut store = GrantStore::default();
            store.grant(grant());
            store
        };
        let executable = Path::new("/usr/bin/rust-analyzer");
        assert!(
            authorize(authorization(
                &descriptor(),
                &store,
                Capability::Lsp,
                executable
            ))
            .is_ok()
        );

        let mut changed = descriptor();
        changed.version = "1.0.1".to_owned();
        assert_eq!(
            authorize(authorization(&changed, &store, Capability::Lsp, executable)),
            Err(Denial::MissingGrant)
        );
        assert_eq!(
            authorize(authorization(
                &descriptor(),
                &store,
                Capability::Lsp,
                Path::new("/usr/bin/other")
            )),
            Err(Denial::MissingGrant)
        );
        assert_eq!(
            authorize(authorization(
                &descriptor(),
                &GrantStore::default(),
                Capability::Lsp,
                executable
            )),
            Err(Denial::MissingGrant)
        );
    }

    #[test]
    fn trust_chain_denies_early() {
        let store = GrantStore::default();
        let descriptor = descriptor();
        let executable = Path::new("/usr/bin/rust-analyzer");
        let mut request = authorization(&descriptor, &store, Capability::Lsp, executable);
        request.installed = false;
        assert_eq!(authorize(request), Err(Denial::NotInstalled));
        let mut request = authorization(&descriptor, &store, Capability::Lsp, executable);
        request.enabled = false;
        assert_eq!(authorize(request), Err(Denial::NotEnabled));
        let mut request = authorization(&descriptor, &store, Capability::Lsp, executable);
        request.workspace_trusted = false;
        assert_eq!(authorize(request), Err(Denial::UntrustedWorkspace));
        assert_eq!(
            authorize(authorization(
                &descriptor,
                &store,
                Capability::Task,
                executable
            )),
            Err(Denial::UnsupportedCapability)
        );
    }

    #[test]
    fn grant_rejects_mismatched_capability() {
        let error = Grant::issue(
            &descriptor(),
            "/work",
            Capability::Task,
            "/usr/bin/rust-analyzer",
        )
        .expect_err("mismatch");
        assert!(error.contains("capability"));
    }

    #[test]
    fn revoke_removes_only_the_bound_grant() {
        let mut store = GrantStore::default();
        store.grant(grant());
        store.revoke("rust-analyzer", Path::new("/work"), Capability::Lsp);
        assert_eq!(
            authorize(authorization(
                &descriptor(),
                &store,
                Capability::Lsp,
                Path::new("/usr/bin/rust-analyzer")
            )),
            Err(Denial::MissingGrant)
        );
    }
}
