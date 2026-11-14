//! Bounded stdin-to-stdout formatters executed through the gateway.

use anvil_ipc::{ClientError, GatewayClient};

const FORMAT_TIMEOUT_MS: u64 = 5_000;
const FORMAT_MAX_BYTES: u64 = 8 * 1024 * 1024;

fn formatter(language_id: &str) -> Option<(String, Vec<String>)> {
    match language_id {
        "rust" => Some((
            existing_tool(&["/usr/bin/rustfmt", "/usr/local/bin/rustfmt"])?,
            vec![
                "--emit".to_owned(),
                "stdout".to_owned(),
                "--edition".to_owned(),
                "2024".to_owned(),
            ],
        )),
        "go" => Some((
            existing_tool(&["/usr/bin/gofmt", "/usr/local/bin/gofmt"])?,
            Vec::new(),
        )),
        _ => None,
    }
}

fn existing_tool(candidates: &[&'static str]) -> Option<String> {
    #[cfg(test)]
    return Some(candidates[0].to_owned());
    #[cfg(not(test))]
    candidates
        .iter()
        .copied()
        .find(|candidate| std::path::Path::new(candidate).is_file())
        .map(str::to_owned)
}

pub(super) fn format_text(
    client: &mut GatewayClient,
    language_id: &str,
    text: &str,
) -> Result<Option<String>, String> {
    let Some((tool, args)) = formatter(language_id) else {
        return Ok(None);
    };
    let result = match client.exec_v2(
        &tool,
        &args,
        None,
        Some(text),
        FORMAT_TIMEOUT_MS,
        Some(FORMAT_MAX_BYTES),
    ) {
        Ok(result) => result,
        Err(ClientError::Rpc {
            code: -32008 | -32002 | -32001,
            ..
        }) => {
            return Ok(None);
        }
        Err(error) => return Err(error.to_string()),
    };
    if result.timed_out {
        return Err("formatter timed out".to_owned());
    }
    if result.output_truncated {
        return Err("formatter output truncated".to_owned());
    }
    if result.exit_code != 0 {
        return Err(format!(
            "formatter exited {}: {}",
            result.exit_code, result.stderr
        ));
    }
    Ok(Some(result.stdout))
}
