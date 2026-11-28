//! Environment and installed-layout diagnostics for `anvil doctor`.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::sandbox::{self, SandboxProfile, SandboxReport};
use crate::{Distribution, RunError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DoctorProfileChoice {
    Auto,
    Dev,
    Installed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DoctorProfile {
    Dev,
    Installed,
}

impl DoctorProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Installed => "installed",
        }
    }
}

impl DoctorProfileChoice {
    fn resolve(self) -> DoctorProfile {
        match self {
            Self::Dev => DoctorProfile::Dev,
            Self::Installed => DoctorProfile::Installed,
            Self::Auto if installed_layout_root().is_some() => DoctorProfile::Installed,
            Self::Auto => DoctorProfile::Dev,
        }
    }
}

fn installed_layout_root() -> Option<PathBuf> {
    let executable = env::current_exe().ok()?;
    let bin = executable.parent()?;
    if bin.file_name().and_then(|name| name.to_str()) != Some("bin") {
        return None;
    }
    let prefix = bin.parent()?;
    prefix
        .join("share/anvil")
        .is_dir()
        .then(|| prefix.to_path_buf())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DoctorLevel {
    Ok,
    Warn,
    Error,
}

impl DoctorLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

struct DoctorCheck {
    id: &'static str,
    level: DoctorLevel,
    message: String,
    remediation: String,
}

fn check(
    id: &'static str,
    level: DoctorLevel,
    message: impl Into<String>,
    remediation: impl Into<String>,
) -> DoctorCheck {
    DoctorCheck {
        id,
        level,
        message: message.into(),
        remediation: remediation.into(),
    }
}

pub(super) fn run_doctor(
    distribution: &Distribution,
    json: bool,
    profile_choice: DoctorProfileChoice,
    sandbox_profile: SandboxProfile,
) -> Result<i32, RunError> {
    let profile = profile_choice.resolve();
    let binary_dir = env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf));
    let sandbox = sandbox::probe(sandbox_profile, binary_dir.as_deref());
    let checks = doctor_checks(profile, binary_dir.as_deref(), &sandbox);
    let errors = checks
        .iter()
        .filter(|check| check.level == DoctorLevel::Error)
        .count();
    let exit_code = doctor_exit_code(&checks);
    if json {
        println!(
            "{}",
            doctor_json(distribution, profile, &sandbox, &checks, exit_code == 0)
        );
    } else {
        println!("{} doctor ({})", distribution.name, profile.as_str());
        for check in &checks {
            println!(
                "{:<5} {:<16} {}",
                check.level.as_str(),
                check.id,
                check.message
            );
            if !check.remediation.is_empty() {
                println!("      remediation: {}", check.remediation);
            }
        }
        println!();
        if errors > 0 {
            println!("{errors} error(s); fix the failing checks before using Anvil.");
        } else {
            println!("installation checks passed.");
        }
    }
    Ok(exit_code)
}

fn doctor_exit_code(checks: &[DoctorCheck]) -> i32 {
    usize::from(checks.iter().any(|check| check.level == DoctorLevel::Error)) as i32
}

fn doctor_checks(
    profile: DoctorProfile,
    binary_dir: Option<&Path>,
    sandbox: &SandboxReport,
) -> Vec<DoctorCheck> {
    vec![
        check_workspace(),
        check_gateway(profile, binary_dir),
        check_companion(
            "anvil-exec-runner",
            profile,
            binary_dir,
            profile == DoctorProfile::Installed,
        ),
        check_companion("anvil-plugin-host", profile, binary_dir, false),
        check_companion("anvil-ctl", profile, binary_dir, false),
        check_companion("anvil-mcp", profile, binary_dir, false),
        check_locale(),
        check_colors(),
        check_remote(),
        check_runtime_dir(),
        check_config(),
        check_deprecated_settings_env(
            env::vars_os()
                .filter_map(|(name, _)| name.into_string().ok())
                .filter(|name| deprecated_settings_env(name)),
        ),
        check_landlock(sandbox),
        check_sandbox(sandbox),
        check_lsp(),
    ]
}

fn check_sandbox(sandbox: &SandboxReport) -> DoctorCheck {
    if let Some(error) = sandbox.policy_error() {
        return check("sandbox", DoctorLevel::Error, error, "");
    }
    if let Some(warning) = sandbox.startup_warning() {
        return check("sandbox", DoctorLevel::Warn, warning, "");
    }
    let message = format!(
        "profile={} plugin: {}; exec: {}; deployment={}",
        sandbox.profile.as_str(),
        sandbox.plugin_summary(),
        sandbox.exec_summary(),
        sandbox.deployment
    );
    check("sandbox", DoctorLevel::Ok, message, "")
}

fn check_workspace() -> DoctorCheck {
    match env::current_dir() {
        Ok(path) if path.is_dir() => {
            check("workspace", DoctorLevel::Ok, path.display().to_string(), "")
        }
        Ok(path) => check(
            "workspace",
            DoctorLevel::Error,
            format!("not a directory: {}", path.display()),
            "run Anvil from a real workspace directory",
        ),
        Err(error) => check(
            "workspace",
            DoctorLevel::Error,
            error.to_string(),
            "check the current directory permissions",
        ),
    }
}

fn check_gateway(profile: DoctorProfile, binary_dir: Option<&Path>) -> DoctorCheck {
    let path = env::var_os("ANVIL_GATEWAY")
        .map(PathBuf::from)
        .or_else(|| binary_dir.map(|directory| directory.join("anvil-gateway")));
    match path {
        Some(path) => check_binary(
            "gateway",
            profile,
            &path,
            profile == DoctorProfile::Installed,
        ),
        None => check(
            "gateway",
            if profile == DoctorProfile::Installed {
                DoctorLevel::Error
            } else {
                DoctorLevel::Warn
            },
            "cannot locate anvil-gateway",
            "install it beside anvil or set ANVIL_GATEWAY",
        ),
    }
}

fn check_companion(
    id: &'static str,
    profile: DoctorProfile,
    binary_dir: Option<&Path>,
    required: bool,
) -> DoctorCheck {
    let Some(path) = binary_dir.map(|directory| directory.join(id)) else {
        return check(
            id,
            if required {
                DoctorLevel::Error
            } else {
                DoctorLevel::Warn
            },
            "cannot determine the Anvil executable directory",
            "install the complete Anvil release layout",
        );
    };
    check_binary(id, profile, &path, required)
}

fn check_binary(
    id: &'static str,
    profile: DoctorProfile,
    path: &Path,
    required: bool,
) -> DoctorCheck {
    if path.is_file() && is_executable(path) {
        return check(id, DoctorLevel::Ok, path.display().to_string(), "");
    }
    let level = if required {
        DoctorLevel::Error
    } else {
        DoctorLevel::Warn
    };
    let (context, remediation) = match profile {
        DoctorProfile::Dev => (
            "dev target sidecar missing",
            "build the sidecar or use --profile installed for a release layout",
        ),
        DoctorProfile::Installed => (
            "installed sidecar missing",
            "install the complete Anvil release layout",
        ),
    };
    check(
        id,
        level,
        format!("{context}: {}", path.display()),
        remediation,
    )
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn nonempty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn check_locale() -> DoctorCheck {
    let Some(locale) = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .find_map(nonempty_env)
    else {
        return check(
            "locale",
            DoctorLevel::Error,
            "LC_ALL/LC_CTYPE/LANG is unset",
            "export LANG=C.UTF-8",
        );
    };
    let lower = locale.to_ascii_lowercase();
    if lower.contains("utf-8") || lower.contains("utf8") {
        check("locale", DoctorLevel::Ok, locale, "")
    } else {
        check(
            "locale",
            DoctorLevel::Error,
            format!("{locale} is not UTF-8"),
            "export LANG=C.UTF-8 or another UTF-8 locale",
        )
    }
}

fn check_colors() -> DoctorCheck {
    let term = env::var("TERM").unwrap_or_default().to_ascii_lowercase();
    if term == "dumb" {
        return check(
            "truecolor",
            DoctorLevel::Warn,
            "TERM=dumb",
            "run in a real terminal; a full-screen TUI needs cursor control",
        );
    }
    let forced = env::var("ANVIL_TRUECOLOR").ok();
    match forced.as_deref() {
        Some("0") => return check("truecolor", DoctorLevel::Ok, "forced 256-color", ""),
        Some("1") => return check("truecolor", DoctorLevel::Ok, "forced 24-bit", ""),
        _ => {}
    }
    let colorterm = env::var("COLORTERM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if colorterm.contains("truecolor")
        || colorterm.contains("24bit")
        || term.contains("direct")
        || term.contains("truecolor")
    {
        check(
            "truecolor",
            DoctorLevel::Ok,
            format!("detected; COLORTERM={colorterm:?} TERM={term:?}"),
            "",
        )
    } else {
        check(
            "truecolor",
            DoctorLevel::Warn,
            "not detected; using 256 colors",
            "set ANVIL_TRUECOLOR=1 or --truecolor=always if the terminal supports 24-bit color",
        )
    }
}

fn check_remote() -> DoctorCheck {
    let ssh = ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|name| env::var_os(name).is_some());
    check_remote_session(
        ssh,
        &env::var("TERM").unwrap_or_default().to_ascii_lowercase(),
        &env::var("COLORTERM")
            .unwrap_or_default()
            .to_ascii_lowercase(),
        env::var_os("TMUX").is_some(),
    )
}

fn check_remote_session(ssh: bool, term: &str, colorterm: &str, tmux: bool) -> DoctorCheck {
    if !ssh {
        return check("remote", DoctorLevel::Ok, "local session", "");
    }
    let truecolor = colorterm.contains("truecolor")
        || colorterm.contains("24bit")
        || term.contains("direct")
        || term.contains("truecolor");
    let mut notes = Vec::new();
    if term == "dumb" {
        notes.push("TERM=dumb");
    }
    if !truecolor {
        notes.push("COLORTERM was not forwarded");
    }
    if tmux {
        notes.push("inside tmux/screen");
    }
    if notes.is_empty() {
        check(
            "remote",
            DoctorLevel::Ok,
            format!("SSH session; TERM={term:?} COLORTERM={colorterm:?}"),
            "",
        )
    } else {
        check(
            "remote",
            DoctorLevel::Warn,
            format!(
                "SSH session; TERM={term:?} COLORTERM={colorterm:?}; {}",
                notes.join("; ")
            ),
            "use ANVIL_TRUECOLOR=1 or --truecolor=always; configure tmux terminal-features for RGB",
        )
    }
}

fn check_runtime_dir() -> DoctorCheck {
    if let Some(runtime) = nonempty_env("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(runtime).join("anvil");
        if path.exists() && !path.is_dir() {
            return check(
                "runtime-dir",
                DoctorLevel::Error,
                format!("not a directory: {}", path.display()),
                "remove the conflicting path or change XDG_RUNTIME_DIR",
            );
        }
        let level = if path.is_dir() {
            DoctorLevel::Ok
        } else {
            DoctorLevel::Warn
        };
        let state = if level == DoctorLevel::Ok {
            "exists"
        } else {
            "will be created"
        };
        return check(
            "runtime-dir",
            level,
            format!("{} ({state})", path.display()),
            "",
        );
    }
    let Some(home) = nonempty_env("HOME") else {
        return check(
            "runtime-dir",
            DoctorLevel::Error,
            "XDG_RUNTIME_DIR is unset and HOME is unavailable",
            "set XDG_RUNTIME_DIR or HOME",
        );
    };
    let path = PathBuf::from(home).join(".local/state/anvil/runtime/anvil");
    check(
        "runtime-dir",
        DoctorLevel::Warn,
        format!("XDG_RUNTIME_DIR is unset; fallback {}", path.display()),
        "set XDG_RUNTIME_DIR to a private user runtime directory",
    )
}

fn config_path() -> Option<PathBuf> {
    if let Some(path) = nonempty_env("ANVIL_SETTINGS") {
        return Some(PathBuf::from(path));
    }
    if let Some(config) = nonempty_env("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(config).join("anvil/settings.toml"));
    }
    nonempty_env("HOME").map(|home| PathBuf::from(home).join(".config/anvil/settings.toml"))
}

fn check_config() -> DoctorCheck {
    let Some(path) = config_path() else {
        return check(
            "config",
            DoctorLevel::Error,
            "cannot resolve ANVIL_SETTINGS, XDG_CONFIG_HOME, or HOME",
            "set ANVIL_SETTINGS or HOME",
        );
    };
    if path.is_file() {
        check("config", DoctorLevel::Ok, path.display().to_string(), "")
    } else if nonempty_env("ANVIL_SETTINGS").is_some() {
        check(
            "config",
            DoctorLevel::Error,
            format!("ANVIL_SETTINGS does not exist: {}", path.display()),
            "create the file or unset ANVIL_SETTINGS",
        )
    } else {
        check(
            "config",
            DoctorLevel::Warn,
            format!("not found; defaults will be used ({})", path.display()),
            "create the file or run :settings to save defaults",
        )
    }
}

fn check_deprecated_settings_env(names: impl IntoIterator<Item = String>) -> DoctorCheck {
    let mut names = names.into_iter().collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return check(
            "settings-env",
            DoctorLevel::Ok,
            "no deprecated settings environment overrides",
            "",
        );
    }
    check(
        "settings-env",
        DoctorLevel::Warn,
        format!("deprecated overrides: {}", names.join(", ")),
        "move these values to settings.toml or languages.toml",
    )
}

fn deprecated_settings_env(name: &str) -> bool {
    name == "ANVIL_LSP"
        || name.starts_with("ANVIL_LSP_")
        || name == "ANVIL_SEARCH_CASE_SENSITIVE"
        || name == "ANVIL_KEYMAP_PRESET"
        || name.starts_with("ANVIL_EDITOR_")
        || name.starts_with("ANVIL_COMPLETION_")
        || name == "ANVIL_REDUCED_MOTION"
        || name == "ANVIL_HIGH_CONTRAST"
}

fn check_landlock(sandbox: &SandboxReport) -> DoctorCheck {
    #[cfg(target_os = "linux")]
    {
        if sandbox.landlock_present {
            return check("landlock", DoctorLevel::Ok, "Landlock is active", "");
        }
        if sandbox.seccomp_available {
            return check(
                "landlock",
                DoctorLevel::Warn,
                "Landlock unavailable; seccomp fallback is active",
                "enable Landlock in the kernel or accept the documented seccomp fallback",
            );
        }
        check(
            "landlock",
            DoctorLevel::Error,
            "neither Landlock nor seccomp is available",
            "use a Linux kernel with Landlock or seccomp",
        )
    }
    #[cfg(not(target_os = "linux"))]
    check(
        "landlock",
        DoctorLevel::Error,
        "Anvil is Linux-first; sandbox checks are unavailable",
        "run Anvil on Linux with Landlock or seccomp",
    )
}

const LSP_TOOLS: &[&str] = &[
    "rust-analyzer",
    "gopls",
    "pyright",
    "pylsp",
    "typescript-language-server",
    "vtsls",
    "clangd",
    "jdtls",
    "lua-language-server",
    "vscode-json-language-server",
    "yaml-language-server",
    "bash-language-server",
    "marksman",
    "taplo",
    "vscode-html-language-server",
    "vscode-css-language-server",
    "zls",
];

fn check_lsp() -> DoctorCheck {
    let Some(path) = env::var_os("PATH") else {
        return check(
            "lsp",
            DoctorLevel::Warn,
            "PATH is unset; no language servers can be discovered",
            "put language servers on PATH",
        );
    };
    let mut found = Vec::new();
    for directory in env::split_paths(&path) {
        for tool in LSP_TOOLS {
            let candidate = directory.join(tool);
            if candidate.is_file() && is_executable(&candidate) && !found.contains(tool) {
                found.push(*tool);
            }
        }
    }
    if found.is_empty() {
        check(
            "lsp",
            DoctorLevel::Warn,
            format!(
                "no supported language server found on PATH ({} candidates)",
                LSP_TOOLS.len()
            ),
            "install the language servers you use, for example rust-analyzer or gopls",
        )
    } else {
        let missing = LSP_TOOLS.len().saturating_sub(found.len());
        check(
            "lsp",
            DoctorLevel::Ok,
            format!(
                "found {} server(s): {}; {missing} optional server(s) missing",
                found.len(),
                found.join(", ")
            ),
            "",
        )
    }
}

fn doctor_json(
    distribution: &Distribution,
    profile: DoctorProfile,
    sandbox: &SandboxReport,
    checks: &[DoctorCheck],
    ok: bool,
) -> String {
    let mut out = String::from("{\"schema_version\":1,\"anvil_version\":\"");
    out.push_str(&json_escape(env!("CARGO_PKG_VERSION")));
    out.push_str("\",\"profile\":\"");
    out.push_str(profile.as_str());
    out.push_str("\",\"distribution\":\"");
    out.push_str(&json_escape(distribution.name));
    out.push_str("\",\"ok\":");
    out.push_str(if ok { "true" } else { "false" });
    out.push_str(",\"sandbox\":");
    out.push_str(&sandbox.to_json());
    out.push_str(",\"checks\":[");
    for (index, check) in checks.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":\"");
        out.push_str(&json_escape(check.id));
        out.push_str("\",\"status\":\"");
        out.push_str(check.level.as_str());
        out.push_str("\",\"message\":\"");
        out.push_str(&json_escape(&check.message));
        out.push_str("\",\"remediation\":\"");
        out.push_str(&json_escape(&check.remediation));
        out.push_str("\"}");
    }
    out.push_str("]}");
    out
}

pub(super) fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::STANDARD;
    use crate::sandbox::SandboxStatus;

    #[test]
    fn settings_env_deprecation_check_warns_for_known_overrides() {
        assert_eq!(
            check_deprecated_settings_env(Vec::new()).level,
            DoctorLevel::Ok
        );
        let report = check_deprecated_settings_env(vec![
            "ANVIL_EDITOR_SCROLL_MARGIN".to_owned(),
            "ANVIL_LSP_RUST".to_owned(),
        ]);
        assert_eq!(report.level, DoctorLevel::Warn);
        assert!(report.message.contains("ANVIL_EDITOR_SCROLL_MARGIN"));
        assert!(report.message.contains("ANVIL_LSP_RUST"));
        assert!(deprecated_settings_env("ANVIL_KEYMAP_PRESET"));
        assert!(!deprecated_settings_env("ANVIL_SETTINGS"));
    }

    #[test]
    fn landlock_check_uses_the_probed_report() {
        let mut report = crate::sandbox::probe(crate::sandbox::SandboxProfile::Auto, None);
        report.landlock_present = true;
        report.seccomp_available = false;
        assert_eq!(check_landlock(&report).level, DoctorLevel::Ok);

        report.landlock_present = false;
        report.seccomp_available = true;
        assert_eq!(check_landlock(&report).level, DoctorLevel::Warn);

        report.seccomp_available = false;
        assert_eq!(check_landlock(&report).level, DoctorLevel::Error);
    }

    #[test]
    fn doctor_json_has_stable_schema() {
        assert_eq!(json_escape("a\"b\nc"), "a\\\"b\\nc");
        let checks = vec![check(
            "gateway",
            DoctorLevel::Error,
            "missing \"gateway\"",
            "install it",
        )];
        let sandbox = sandbox::probe(SandboxProfile::Auto, None);
        let output = doctor_json(
            &STANDARD,
            DoctorProfile::Installed,
            &sandbox,
            &checks,
            false,
        );
        assert!(output.contains("\"schema_version\":1"));
        assert!(output.contains(&format!(
            "\"anvil_version\":\"{}\"",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(output.contains("\"profile\":\"installed\""));
        assert!(output.contains("\"distribution\":\"anvil\""));
        assert!(output.contains("\"ok\":false"));
        assert!(output.contains("\"sandbox\":"));
        assert!(output.contains("\"plugin\":"));
        assert!(output.contains("\"trust\":\"trusted_external\""));
        assert!(output.contains("\"id\":\"gateway\""));
        assert!(output.contains("\"message\":\"missing \\\"gateway\\\"\""));
        assert!(output.contains("\"remediation\":\"install it\""));
    }

    #[test]
    fn dev_profile_downgrades_missing_sidecars() {
        let directory = tempfile::tempdir().expect("tempdir");
        let dev = check_companion(
            "anvil-exec-runner",
            DoctorProfile::Dev,
            Some(directory.path()),
            false,
        );
        assert_eq!(dev.level, DoctorLevel::Warn);
        let installed = check_companion(
            "anvil-exec-runner",
            DoctorProfile::Installed,
            Some(directory.path()),
            true,
        );
        assert_eq!(installed.level, DoctorLevel::Error);
    }

    #[test]
    fn doctor_exit_code_tracks_required_errors() {
        let directory = tempfile::tempdir().expect("tempdir");
        let dev = vec![check_companion(
            "gateway",
            DoctorProfile::Dev,
            Some(directory.path()),
            false,
        )];
        assert_eq!(doctor_exit_code(&dev), 0);
        let installed = vec![check_companion(
            "gateway",
            DoctorProfile::Installed,
            Some(directory.path()),
            true,
        )];
        assert_eq!(doctor_exit_code(&installed), 1);
    }

    #[test]
    fn installed_profile_accepts_fake_sidecars_in_json() {
        let directory = tempfile::tempdir().expect("tempdir");
        let names = [
            "anvil-gateway",
            "anvil-exec-runner",
            "anvil-plugin-host",
            "anvil-ctl",
            "anvil-mcp",
        ];
        for name in names {
            let path = directory.path().join(name);
            fs::write(&path, "").expect("sidecar");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("permissions");
            }
        }
        let checks = vec![
            check_binary(
                "gateway",
                DoctorProfile::Installed,
                &directory.path().join("anvil-gateway"),
                true,
            ),
            check_companion(
                "anvil-exec-runner",
                DoctorProfile::Installed,
                Some(directory.path()),
                true,
            ),
            check_companion(
                "anvil-plugin-host",
                DoctorProfile::Installed,
                Some(directory.path()),
                false,
            ),
            check_companion(
                "anvil-ctl",
                DoctorProfile::Installed,
                Some(directory.path()),
                false,
            ),
            check_companion(
                "anvil-mcp",
                DoctorProfile::Installed,
                Some(directory.path()),
                false,
            ),
        ];
        assert!(checks.iter().all(|check| check.level == DoctorLevel::Ok));
        assert_eq!(doctor_exit_code(&checks), 0);
        let sandbox = sandbox::probe(SandboxProfile::Auto, None);
        let json = doctor_json(&STANDARD, DoctorProfile::Installed, &sandbox, &checks, true);
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"profile\":\"installed\""));
        assert!(json.contains("\"remediation\":\"\""));
    }

    #[test]
    fn remote_check_distinguishes_ssh_capabilities() {
        assert_eq!(
            check_remote_session(false, "xterm-256color", "", false).level,
            DoctorLevel::Ok
        );
        assert_eq!(
            check_remote_session(true, "xterm-256color", "", false).level,
            DoctorLevel::Warn
        );
        assert_eq!(
            check_remote_session(true, "xterm-direct", "", false).level,
            DoctorLevel::Ok
        );
        assert_eq!(
            check_remote_session(true, "xterm-direct", "truecolor", true).level,
            DoctorLevel::Warn
        );
    }

    #[test]
    fn sandbox_check_marks_degraded_and_disabled() {
        let mut report = sandbox::probe(SandboxProfile::Auto, None);
        report.plugin_status = SandboxStatus::Degraded;
        report.exec_status = SandboxStatus::Strict;
        assert_eq!(check_sandbox(&report).level, DoctorLevel::Warn);

        report.plugin_status = SandboxStatus::Disabled;
        assert_eq!(check_sandbox(&report).level, DoctorLevel::Error);
    }
}
