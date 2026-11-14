//! File enumeration, project search, and project replace helpers.

use super::*;

pub(crate) fn files(
    client: &mut GatewayClient,
    workspace: &Path,
    hidden: bool,
) -> Result<Vec<String>, String> {
    if let Some(tool) = rg_tool() {
        let mut args = vec!["--files".to_owned()];
        if hidden {
            args.push("--hidden".to_owned());
        }
        for excluded in [".git", "target", ".cache", ".cargo", ".rustup"] {
            args.push("--glob".to_owned());
            args.push(format!("!{excluded}/**"));
        }
        if let Ok(result) = client.exec(&tool, &args, 3000)
            && result.exit_code == 0
            && !result.timed_out
        {
            let mut files: Vec<String> = result
                .stdout
                .lines()
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .filter(|line| {
                    ![".git/", "target/", ".cache/", ".cargo/", ".rustup/"]
                        .iter()
                        .any(|prefix| line.starts_with(prefix))
                })
                .collect();
            files.sort();
            files.dedup();
            if files.len() <= FILE_LIMIT {
                return Ok(files);
            }
        }
    }
    let mut files = Vec::new();
    collect_fallback(client, workspace, workspace, "", hidden, &mut files)?;
    files.sort();
    files.dedup();
    Ok(files)
}

pub(crate) fn project_search(
    client: &mut GatewayClient,
    query: &str,
) -> Result<(Vec<PipeLocation>, bool), String> {
    let tool = rg_tool().ok_or_else(|| "rg is unavailable".to_owned())?;
    let result = client
        .exec(
            &tool,
            &[
                "--json".to_owned(),
                "--line-number".to_owned(),
                "--smart-case".to_owned(),
                "--".to_owned(),
                query.to_owned(),
            ],
            5000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("project search timed out".to_owned());
    }
    if result.exit_code > 1 {
        return Err(result.stderr);
    }
    let mut truncated = result.output_truncated;
    let mut locations = Vec::new();
    for line in result.stdout.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if event.get("type").and_then(Value::as_str) != Some("match") {
            continue;
        }
        let path = event["data"]["path"]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let line_number = event["data"]["line_number"].as_u64().unwrap_or(1) as usize;
        let column = event["data"]["submatches"][0]["start"]
            .as_u64()
            .unwrap_or(0) as usize
            + 1;
        let message = event["data"]["lines"]["text"]
            .as_str()
            .unwrap_or_default()
            .trim_end()
            .to_owned();
        if path.is_empty() {
            continue;
        }
        if locations.len() >= MAX_PROJECT_RESULTS {
            truncated = true;
            break;
        }
        locations.push(PipeLocation {
            path,
            line: line_number,
            column,
            message,
        });
    }
    Ok((locations, truncated))
}

pub(crate) fn project_replace_preview(
    client: &mut GatewayClient,
    old: &str,
    new: &str,
) -> Result<ProjectReplacePlan, String> {
    if old.is_empty() {
        return Err("replace search text is empty".to_owned());
    }
    if old == new {
        return Err("replacement is identical to search text".to_owned());
    }
    let tool = rg_tool().ok_or_else(|| "rg is unavailable".to_owned())?;
    let result = client
        .exec(
            &tool,
            &[
                "--files-with-matches".to_owned(),
                "--fixed-strings".to_owned(),
                "--".to_owned(),
                old.to_owned(),
            ],
            5000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("replace preview timed out".to_owned());
    }
    if result.exit_code > 1 {
        return Err(result.stderr);
    }
    if result.output_truncated {
        return Err("too many files for project replace".to_owned());
    }
    let mut files = Vec::new();
    let mut total = 0usize;
    for (index, relative) in result.stdout.lines().enumerate() {
        let relative = relative.trim();
        if relative.is_empty() {
            continue;
        }
        if index >= MAX_REPLACE_FILES {
            return Err(format!(
                "project replace is limited to {MAX_REPLACE_FILES} files"
            ));
        }
        let bytes = client
            .read_all_binary(relative, MAX_REPLACE_FILE_BYTES)
            .map_err(|error| format!("{relative}: {error}"))?;
        total = total.saturating_add(bytes.len());
        if total > MAX_REPLACE_BYTES {
            return Err(format!(
                "project replace is limited to {MAX_REPLACE_BYTES} bytes"
            ));
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let matches = text.matches(old).count();
        if matches == 0 {
            continue;
        }
        files.push(ProjectReplaceFile {
            relative: relative.to_owned(),
            matches,
            hash: Sha256::digest(&bytes).into(),
        });
    }
    Ok(ProjectReplacePlan {
        old: old.to_owned(),
        new: new.to_owned(),
        files,
    })
}

pub(crate) fn apply_project_replace(
    client: &mut GatewayClient,
    plan: &ProjectReplacePlan,
) -> Result<(), String> {
    let mut originals = Vec::with_capacity(plan.files.len());
    let mut writes = Vec::with_capacity(plan.files.len());
    for file in &plan.files {
        let bytes = client
            .read_all_binary(&file.relative, MAX_REPLACE_FILE_BYTES)
            .map_err(|error| format!("{}: {error}", file.relative))?;
        let hash: [u8; 32] = Sha256::digest(&bytes).into();
        if hash != file.hash {
            return Err(format!(
                "{} changed since the preview; rerun the preview",
                file.relative
            ));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| format!("{} is not valid UTF-8", file.relative))?;
        let replaced = text.replace(&plan.old, &plan.new);
        originals.push((file.relative.clone(), bytes));
        writes.push((file.relative.clone(), replaced.into_bytes()));
    }
    for (index, (relative, data)) in writes.iter().enumerate() {
        if let Err(error) = client.write_all(relative, data) {
            let mut rollback_error = None;
            for (original_path, original) in originals.iter().take(index + 1) {
                if let Err(rollback) = client.write_all(original_path, original) {
                    rollback_error = Some(format!("{original_path}: {rollback}"));
                }
            }
            return Err(match rollback_error {
                Some(rollback) => {
                    format!("{relative}: {error}; rollback failed: {rollback}")
                }
                None => format!("{relative}: {error}; rollback complete"),
            });
        }
    }
    Ok(())
}

fn rg_tool() -> Option<String> {
    if let Some(path) = std::env::var_os("ANVIL_RG") {
        return Some(path.to_string_lossy().into_owned());
    }
    ["/usr/bin/rg", "/bin/rg"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .map(ToOwned::to_owned)
}

fn collect_fallback(
    client: &mut GatewayClient,
    root: &Path,
    directory: &Path,
    prefix: &str,
    hidden: bool,
    out: &mut Vec<String>,
) -> Result<(), String> {
    if out.len() >= FILE_LIMIT {
        return Ok(());
    }
    let relative = directory
        .strip_prefix(root)
        .unwrap_or(directory)
        .to_string_lossy()
        .into_owned();
    let relative = if relative.is_empty() {
        ".".to_owned()
    } else {
        relative
    };
    let entries = client
        .read_dir(&relative)
        .map_err(|error| error.to_string())?;
    for entry in entries {
        if [".git", "target", ".cache", ".cargo", ".rustup"].contains(&entry.name.as_str()) {
            continue;
        }
        if !hidden && entry.name.starts_with('.') {
            continue;
        }
        let child = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{prefix}/{}", entry.name)
        };
        if entry.is_dir {
            collect_fallback(
                client,
                root,
                &directory.join(&entry.name),
                &child,
                hidden,
                out,
            )?;
        } else {
            out.push(child);
        }
        if out.len() >= FILE_LIMIT {
            break;
        }
    }
    Ok(())
}
