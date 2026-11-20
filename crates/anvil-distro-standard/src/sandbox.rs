//! Structured sandbox and trust-boundary capability report.

use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SandboxStatus {
    Strict,
    Degraded,
    Disabled,
}

impl SandboxStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Degraded => "degraded",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SandboxProfile {
    Auto,
    Strict,
    Off,
}

impl SandboxProfile {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Strict => "strict",
            Self::Off => "off",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "strict" => Ok(Self::Strict),
            "off" => Ok(Self::Off),
            _ => Err(format!(
                "invalid sandbox profile {value:?}; expected auto, strict, or off"
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct SandboxReport {
    pub(super) profile: SandboxProfile,
    pub(super) landlock_present: bool,
    pub(super) landlock_abi: Option<u32>,
    pub(super) signal_scope: bool,
    pub(super) seccomp_available: bool,
    pub(super) cgroup_v2: bool,
    pub(super) plugin_status: SandboxStatus,
    pub(super) runner: Option<PathBuf>,
    pub(super) exec_status: SandboxStatus,
    pub(super) deployment: &'static str,
}

impl SandboxReport {
    pub(super) fn plugin_summary(&self) -> String {
        let abi = self
            .landlock_abi
            .map_or_else(|| "none".to_owned(), |abi| abi.to_string());
        format!(
            "landlock={} abi={abi} signal_scope={} seccomp={} cgroup_v2={} status={}",
            self.landlock_present,
            self.signal_scope,
            self.seccomp_available,
            self.cgroup_v2,
            self.plugin_status.as_str()
        )
    }

    pub(super) fn exec_summary(&self) -> String {
        let runner = self
            .runner
            .as_ref()
            .map_or_else(|| "missing".to_owned(), |path| path.display().to_string());
        format!(
            "policy=gateway runner={runner} status={}",
            self.exec_status.as_str()
        )
    }

    pub(super) fn policy_error(&self) -> Option<String> {
        if self.plugin_status == SandboxStatus::Disabled {
            return Some("untrusted plugins cannot start without Landlock or seccomp".to_owned());
        }
        if self.profile == SandboxProfile::Strict {
            if self.plugin_status != SandboxStatus::Strict {
                return Some(
                    "sandbox=strict requires the full plugin boundary; use auto or off explicitly"
                        .to_owned(),
                );
            }
            if self.exec_status != SandboxStatus::Strict {
                return Some(
                    "sandbox=strict requires anvil-exec-runner and the full exec boundary"
                        .to_owned(),
                );
            }
        }
        None
    }

    pub(super) fn startup_warning(&self) -> Option<String> {
        if self.profile == SandboxProfile::Off {
            return Some(
                "sandbox=off is a development escape hatch; untrusted plugin boundaries remain enforced"
                    .to_owned(),
            );
        }
        if self.plugin_status == SandboxStatus::Degraded {
            return Some("sandbox is degraded; plugin signal isolation may be reduced".to_owned());
        }
        if self.exec_status == SandboxStatus::Disabled {
            return Some("sandbox exec is disabled: anvil-exec-runner was not found".to_owned());
        }
        None
    }

    pub(super) fn to_json(&self) -> String {
        serde_json::json!({
            "profile": self.profile.as_str(),
            "plugin": {
                "landlock": {
                    "present": self.landlock_present,
                    "abi": self.landlock_abi,
                    "signal_scope": self.signal_scope,
                },
                "seccomp": {
                    "available": self.seccomp_available,
                    "mode": if self.seccomp_available { "denylist" } else { "unavailable" },
                },
                "cgroup_v2": self.cgroup_v2,
                "status": self.plugin_status.as_str(),
            },
            "exec": {
                "policy": "gateway",
                "runner": {
                    "found": self.runner.is_some(),
                    "path": self.runner.as_ref().map(|path| path.display().to_string()),
                },
                "status": self.exec_status.as_str(),
            },
            "lsp": {
                "sandboxed": false,
                "trust": "trusted_external",
            },
            "control": {
                "transport": "unix",
                "peer": "same_uid",
                "remote": false,
            },
            "deployment": self.deployment,
        })
        .to_string()
    }
}

pub(super) fn probe(profile: SandboxProfile, binary_dir: Option<&Path>) -> SandboxReport {
    let (landlock_present, landlock_abi) = landlock_probe();
    let signal_scope = landlock_abi.is_some_and(|abi| abi >= 6);
    let seccomp_available = seccomp_available();
    let plugin_status = plugin_status(landlock_present, seccomp_available);
    let runner = find_exec_runner(binary_dir);
    let exec_status = if runner.is_none() {
        SandboxStatus::Disabled
    } else if landlock_present && seccomp_available {
        SandboxStatus::Strict
    } else {
        SandboxStatus::Degraded
    };

    SandboxReport {
        profile,
        landlock_present,
        landlock_abi,
        signal_scope,
        seccomp_available,
        cgroup_v2: Path::new("/sys/fs/cgroup/cgroup.controllers").is_file(),
        plugin_status,
        runner,
        exec_status,
        deployment: detect_deployment(),
    }
}

fn plugin_status(landlock_present: bool, seccomp_available: bool) -> SandboxStatus {
    match (landlock_present, seccomp_available) {
        (true, true) => SandboxStatus::Strict,
        (true, false) | (false, true) => SandboxStatus::Degraded,
        (false, false) => SandboxStatus::Disabled,
    }
}

#[cfg(target_os = "linux")]
fn landlock_probe() -> (bool, Option<u32>) {
    const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;
    let abi = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<libc::c_void>(),
            0usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    if abi < 1 {
        (false, None)
    } else {
        (true, u32::try_from(abi).ok())
    }
}

#[cfg(not(target_os = "linux"))]
fn landlock_probe() -> (bool, Option<u32>) {
    (false, None)
}

fn seccomp_available() -> bool {
    Path::new("/proc/sys/kernel/seccomp/actions_avail").is_file()
}

fn find_exec_runner(binary_dir: Option<&Path>) -> Option<PathBuf> {
    let explicit = env::var_os("ANVIL_EXEC_RUNNER").map(PathBuf::from);
    anvil_process::find_executable("anvil-exec-runner", explicit.as_deref(), binary_dir)
}

fn detect_deployment() -> &'static str {
    if env::var_os("FLATPAK_ID").is_some() || Path::new("/.flatpak-info").is_file() {
        "flatpak"
    } else if env::var_os("SNAP").is_some() || env::var_os("SNAP_NAME").is_some() {
        "snap"
    } else if Path::new("/.dockerenv").is_file() || env::var_os("container").is_some() {
        "container"
    } else if env::var_os("INVOCATION_ID").is_some() || env::var_os("NOTIFY_SOCKET").is_some() {
        "service"
    } else if env::var_os("SSH_CONNECTION").is_some()
        || env::var_os("SSH_CLIENT").is_some()
        || env::var_os("SSH_TTY").is_some()
    {
        "ssh"
    } else {
        "local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_status_never_claims_strict_without_both_boundaries() {
        assert_eq!(plugin_status(true, true), SandboxStatus::Strict);
        assert_eq!(plugin_status(true, false), SandboxStatus::Degraded);
        assert_eq!(plugin_status(false, true), SandboxStatus::Degraded);
        assert_eq!(plugin_status(false, false), SandboxStatus::Disabled);
    }

    #[test]
    fn report_json_contains_the_required_boundaries() {
        let report = probe(SandboxProfile::Auto, None);
        let json: serde_json::Value =
            serde_json::from_str(&report.to_json()).expect("sandbox json");
        assert_eq!(json["profile"], "auto");
        assert!(json["plugin"]["landlock"]["present"].is_boolean());
        assert!(json["plugin"]["seccomp"]["available"].is_boolean());
        assert!(json["plugin"]["status"].is_string());
        assert!(json["exec"]["runner"]["found"].is_boolean());
        assert_eq!(json["lsp"]["sandboxed"], false);
        assert_eq!(json["lsp"]["trust"], "trusted_external");
        assert_eq!(json["control"]["peer"], "same_uid");
        assert!(json["deployment"].is_string());
    }

    #[test]
    fn sandbox_profile_parses_and_rejects_unknown_values() {
        assert_eq!(SandboxProfile::parse("AUTO"), Ok(SandboxProfile::Auto));
        assert_eq!(SandboxProfile::parse("strict"), Ok(SandboxProfile::Strict));
        assert_eq!(SandboxProfile::parse("off"), Ok(SandboxProfile::Off));
        assert!(SandboxProfile::parse("permissive").is_err());
    }

    #[test]
    fn strict_profile_rejects_degraded_boundaries_and_off_warns() {
        let mut report = probe(SandboxProfile::Strict, None);
        report.plugin_status = SandboxStatus::Degraded;
        report.exec_status = SandboxStatus::Strict;
        assert!(report.policy_error().is_some());

        report.profile = SandboxProfile::Off;
        assert!(report.policy_error().is_none());
        assert!(report.startup_warning().is_some());
    }
}
