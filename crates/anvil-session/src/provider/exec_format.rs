use super::{CancellationToken, FormatContext, FormatterProvider, ProviderError, ProviderId};
use std::sync::Arc;

/// Result of one bounded formatter process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i64,
    pub timed_out: bool,
    pub output_truncated: bool,
}

/// Runs one formatter through a bounded exec boundary.
pub trait ExecV2: Send + Sync {
    fn exec_v2(
        &self,
        tool: &str,
        args: &[String],
        cwd: Option<&str>,
        stdin: &str,
        timeout_ms: u64,
        max_output_bytes: u64,
    ) -> Result<ExecOutput, ProviderError>;
}

/// A stdin-to-stdout formatter backed by one allowlisted exec tool.
pub struct ExecFormatter {
    id: ProviderId,
    language_id: String,
    tool: String,
    args: Vec<String>,
    cwd: Option<String>,
    timeout_ms: u64,
    max_output_bytes: u64,
    exec: Arc<dyn ExecV2>,
}

impl ExecFormatter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ProviderId,
        language_id: impl Into<String>,
        tool: impl Into<String>,
        args: Vec<String>,
        cwd: Option<String>,
        timeout_ms: u64,
        max_output_bytes: u64,
        exec: Arc<dyn ExecV2>,
    ) -> Self {
        Self {
            id,
            language_id: language_id.into(),
            tool: tool.into(),
            args,
            cwd,
            timeout_ms,
            max_output_bytes,
            exec,
        }
    }
}

impl FormatterProvider for ExecFormatter {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }

    fn is_available(&self, context: &FormatContext<'_>) -> bool {
        context.language_id == self.language_id
    }

    fn format(
        &self,
        context: &FormatContext<'_>,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, ProviderError> {
        cancel.check()?;
        let output = self.exec.exec_v2(
            &self.tool,
            &self.args,
            self.cwd.as_deref(),
            context.text,
            self.timeout_ms,
            self.max_output_bytes,
        )?;
        cancel.check()?;
        if output.timed_out {
            return Err(ProviderError::Timeout);
        }
        if output.output_truncated {
            return Err(ProviderError::InvalidData(
                "formatter output truncated".to_owned(),
            ));
        }
        if output.exit_code != 0 {
            return Err(ProviderError::Internal(format!(
                "formatter exited {}: {}",
                output.exit_code, output.stderr
            )));
        }
        Ok(Some(output.stdout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    struct FakeExec {
        output: ExecOutput,
    }

    impl ExecV2 for FakeExec {
        fn exec_v2(
            &self,
            _tool: &str,
            _args: &[String],
            _cwd: Option<&str>,
            _stdin: &str,
            _timeout_ms: u64,
            _max_output_bytes: u64,
        ) -> Result<ExecOutput, ProviderError> {
            Ok(self.output.clone())
        }
    }

    fn formatter(output: ExecOutput) -> ExecFormatter {
        ExecFormatter::new(
            ProviderId::new("test.exec"),
            "rust",
            "rustfmt",
            vec!["--emit".to_owned(), "stdout".to_owned()],
            None,
            1000,
            1 << 20,
            Arc::new(FakeExec { output }),
        )
    }

    fn context() -> FormatContext<'static> {
        FormatContext {
            path: Some(Path::new("src/main.rs")),
            language_id: "rust",
            revision: 1,
            text: "fn main(){}",
        }
    }

    fn output(stdout: &str, exit_code: i64) -> ExecOutput {
        ExecOutput {
            stdout: stdout.to_owned(),
            stderr: String::new(),
            exit_code,
            timed_out: false,
            output_truncated: false,
        }
    }

    #[test]
    fn successful_output_is_returned() {
        let formatted = formatter(output("fn main() {}\n", 0))
            .format(&context(), &CancellationToken::new())
            .expect("format");
        assert_eq!(formatted.as_deref(), Some("fn main() {}\n"));
    }

    #[test]
    fn formatter_is_language_scoped() {
        let formatter = formatter(output("formatted", 0));
        let go = FormatContext {
            path: Some(Path::new("main.go")),
            language_id: "go",
            revision: 1,
            text: "package main",
        };
        assert!(!formatter.is_available(&go));
        assert!(formatter.is_available(&context()));
    }

    #[test]
    fn failed_processes_map_to_provider_errors() {
        assert_eq!(
            formatter(output("", 2)).format(&context(), &CancellationToken::new()),
            Err(ProviderError::Internal("formatter exited 2: ".to_owned()))
        );
        let mut timed_out = output("", 0);
        timed_out.timed_out = true;
        assert_eq!(
            formatter(timed_out).format(&context(), &CancellationToken::new()),
            Err(ProviderError::Timeout)
        );
        let mut truncated = output("partial", 0);
        truncated.output_truncated = true;
        assert_eq!(
            formatter(truncated).format(&context(), &CancellationToken::new()),
            Err(ProviderError::InvalidData(
                "formatter output truncated".to_owned()
            ))
        );
    }
}
