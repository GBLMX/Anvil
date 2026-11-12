//! Git status, diff, blame, conflict, and hunk helpers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anvil_ipc::GatewayClient;

use super::ResolveConflictError;

pub(crate) fn git_status(
    client: &mut GatewayClient,
    workspace: &Path,
) -> Result<HashMap<PathBuf, char>, String> {
    let result = client
        .exec(
            &git_tool(),
            &[
                "--no-pager".to_owned(),
                "status".to_owned(),
                "--porcelain=v1".to_owned(),
                "--relative".to_owned(),
                "-z".to_owned(),
            ],
            3000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git status timed out".to_owned());
    }
    if result.exit_code > 1 {
        return Err(result.stderr);
    }
    Ok(parse_git_status(&result.stdout, workspace))
}

pub(crate) fn parse_git_status(output: &str, workspace: &Path) -> HashMap<PathBuf, char> {
    let mut statuses = HashMap::new();
    let mut entries = output
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .peekable();
    while let Some(entry) = entries.next() {
        if entry.len() < 3 {
            continue;
        }
        let code = &entry[..2];
        let path = &entry[3..];
        if path.is_empty() {
            continue;
        }
        let marker = if code.contains('?') || code.contains('A') {
            'A'
        } else if code.contains('D') {
            'D'
        } else if code.contains('R') {
            'R'
        } else if code.contains('C') {
            'C'
        } else {
            'M'
        };
        statuses.insert(workspace.join(path), marker);
        if code.contains('R') || code.contains('C') {
            let _ = entries.next();
        }
    }
    statuses
}

fn git_tool() -> String {
    std::env::var_os("ANVIL_GIT")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/usr/bin/git".to_owned())
}

pub(crate) fn git_diff(
    client: &mut GatewayClient,
    relative: &str,
) -> Result<Vec<(usize, char)>, String> {
    let result = client
        .exec(
            &git_tool(),
            &[
                "--no-pager".to_owned(),
                "diff".to_owned(),
                "--no-ext-diff".to_owned(),
                "--no-textconv".to_owned(),
                "--no-color".to_owned(),
                "--unified=0".to_owned(),
                "HEAD".to_owned(),
                "--".to_owned(),
                relative.to_owned(),
            ],
            3000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git diff timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    Ok(parse_git_diff(&result.stdout))
}

const MAX_GIT_DIFF_BYTES: usize = 512 * 1024;

pub(crate) fn git_diff_text(
    client: &mut GatewayClient,
    relative: &str,
    staged: bool,
) -> Result<(String, bool), String> {
    let mut args = vec!["--no-pager".to_owned(), "diff".to_owned()];
    if staged {
        args.push("--cached".to_owned());
    }
    args.extend([
        "--no-ext-diff".to_owned(),
        "--no-textconv".to_owned(),
        "--no-color".to_owned(),
        "--unified=3".to_owned(),
        "HEAD".to_owned(),
        "--".to_owned(),
        relative.to_owned(),
    ]);
    let result = client
        .exec(&git_tool(), &args, 5000)
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git diff timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    let mut text = result.stdout;
    let mut truncated = result.output_truncated;
    if text.len() > MAX_GIT_DIFF_BYTES {
        let mut end = MAX_GIT_DIFF_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    if text.is_empty() {
        text.push_str("no changes relative to HEAD\n");
    }
    Ok((text, truncated))
}

pub(crate) fn git_blame_text(
    client: &mut GatewayClient,
    relative: &str,
) -> Result<(String, bool), String> {
    let result = client
        .exec(
            &git_tool(),
            &[
                "--no-pager".to_owned(),
                "blame".to_owned(),
                "--date=short".to_owned(),
                "--".to_owned(),
                relative.to_owned(),
            ],
            5000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git blame timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    let mut text = result.stdout;
    let mut truncated = result.output_truncated;
    if text.len() > MAX_GIT_DIFF_BYTES {
        let mut end = MAX_GIT_DIFF_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    if text.is_empty() {
        text.push_str("no blame data\n");
    }
    Ok((text, truncated))
}

pub(crate) fn git_conflicts_text(client: &mut GatewayClient) -> Result<(String, bool), String> {
    let result = client
        .exec(
            &git_tool(),
            &[
                "--no-pager".to_owned(),
                "diff".to_owned(),
                "--name-status".to_owned(),
                "--diff-filter=U".to_owned(),
            ],
            3000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git conflicts timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    let mut text = result.stdout;
    let mut truncated = result.output_truncated;
    if text.len() > MAX_GIT_DIFF_BYTES {
        let mut end = MAX_GIT_DIFF_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    if text.is_empty() {
        text.push_str("no conflicts\n");
    }
    Ok((text, truncated))
}

pub(crate) fn git_conflict_stage_text(
    client: &mut GatewayClient,
    relative: &str,
    stage: u8,
) -> Result<(String, bool), String> {
    if !(1..=3).contains(&stage) {
        return Err("invalid conflict stage".to_owned());
    }
    let spec = format!(":{stage}:{relative}");
    let result = client
        .exec(
            &git_tool(),
            &["--no-pager".to_owned(), "show".to_owned(), spec],
            5000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git show timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    let mut text = result.stdout;
    let mut truncated = result.output_truncated;
    if text.len() > MAX_GIT_DIFF_BYTES {
        let mut end = MAX_GIT_DIFF_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    if text.is_empty() {
        text.push_str("stage is empty\n");
    }
    Ok((text, truncated))
}

pub(crate) fn resolve_git_conflict(
    client: &mut GatewayClient,
    relative: &str,
    stage: u8,
) -> Result<(), ResolveConflictError> {
    if !(2..=3).contains(&stage) {
        return Err(ResolveConflictError::before_write(
            "conflict resolution requires stage 2 or 3",
        ));
    }
    let spec = format!(":{stage}:{relative}");
    let show = client
        .exec(
            &git_tool(),
            &["--no-pager".to_owned(), "show".to_owned(), spec],
            5000,
        )
        .map_err(|error| ResolveConflictError::before_write(error.to_string()))?;
    if show.timed_out {
        return Err(ResolveConflictError::before_write("git show timed out"));
    }
    if show.exit_code != 0 {
        return Err(ResolveConflictError::before_write(show.stderr));
    }
    if show.output_truncated {
        return Err(ResolveConflictError::before_write(
            "conflict stage is too large to resolve automatically",
        ));
    }
    client
        .write_all(relative, show.stdout.as_bytes())
        .map_err(|error| ResolveConflictError::after_write(error.to_string()))?;
    let add = client
        .exec(
            &git_tool(),
            &[
                "--no-pager".to_owned(),
                "add".to_owned(),
                "--".to_owned(),
                relative.to_owned(),
            ],
            5000,
        )
        .map_err(|error| ResolveConflictError::after_write(error.to_string()))?;
    if add.timed_out {
        return Err(ResolveConflictError::after_write("git add timed out"));
    }
    if add.exit_code != 0 {
        return Err(ResolveConflictError::after_write(add.stderr));
    }
    Ok(())
}

const GIT_HUNK_PATCH: &str = ".anvil-tmp/git-hunk.patch";

pub(crate) fn apply_git_hunk(
    client: &mut GatewayClient,
    patch: &str,
    reverse: bool,
) -> Result<(), String> {
    client
        .write_all(GIT_HUNK_PATCH, patch.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut args = vec![
        "--no-pager".to_owned(),
        "apply".to_owned(),
        "--cached".to_owned(),
        "--whitespace=nowarn".to_owned(),
    ];
    if reverse {
        args.push("-R".to_owned());
    }
    args.extend(["--".to_owned(), GIT_HUNK_PATCH.to_owned()]);
    let result = client
        .exec(&git_tool(), &args, 5000)
        .map_err(|error| error.to_string());
    let _ = client.remove(GIT_HUNK_PATCH);
    let result = result?;
    if result.timed_out {
        return Err("git apply timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    Ok(())
}

pub(crate) fn parse_git_diff(output: &str) -> Vec<(usize, char)> {
    let mut marks = Vec::new();
    for line in output.lines() {
        if !line.starts_with("@@ ") {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(old_range) = fields.nth(1) else {
            continue;
        };
        let Some(new_range) = fields.next() else {
            continue;
        };
        let Some(old_range) = old_range.strip_prefix('-') else {
            continue;
        };
        let Some(new_range) = new_range.strip_prefix('+') else {
            continue;
        };
        let mut old_parts = old_range.split(',');
        let _old_start = old_parts.next();
        let old_count = old_parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        let mut new_parts = new_range.split(',');
        let Some(start) = new_parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
        else {
            continue;
        };
        let count = new_parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        if count == 0 {
            marks.push((start.saturating_sub(1), '-'));
        } else {
            let marker = if old_count == 0 { '+' } else { '~' };
            for offset in 0..count {
                marks.push((start.saturating_sub(1) + offset, marker));
            }
        }
    }
    marks.sort_unstable();
    marks.dedup();
    marks
}
