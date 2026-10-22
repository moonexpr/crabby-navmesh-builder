//! Machine-readable output for the command-line tools: TOON by default,
//! JSON on request. Both render the same serializable report values, and a
//! failure is a report too.

use std::process::ExitCode;

use serde::Serialize;

use super::{Failure, FailureKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// Token-Oriented Object Notation, compact for language-model agents.
    #[default]
    Toon,
    /// Pretty-printed JSON.
    Json,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("could not render TOON: {0}")]
    Toon(String),
    #[error("could not render JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// The report as text ending in a newline.
pub fn render<T: Serialize>(value: &T, format: Format) -> Result<String, RenderError> {
    let mut text = match format {
        Format::Toon => toon_format::encode_default(value).map_err(|e| RenderError::Toon(e.to_string()))?,
        Format::Json => serde_json::to_string_pretty(value)?,
    };
    text.push('\n');
    Ok(text)
}

impl FailureKind {
    /// 2 for the caller's fault, 1 for ours.
    pub fn exit_code(self) -> ExitCode {
        match self {
            FailureKind::Input => ExitCode::from(2),
            FailureKind::Internal => ExitCode::from(1),
        }
    }
}

/// How a failure is reported on stdout.
#[derive(Debug, PartialEq, Serialize)]
pub struct FailureReport {
    pub status: &'static str,
    pub kind: FailureKind,
    pub message: String,
    /// The error's causes, outermost first, after the message.
    pub causes: Vec<String>,
}

impl From<&Failure> for FailureReport {
    fn from(failure: &Failure) -> Self {
        let mut chain = failure.error.chain().map(ToString::to_string);
        Self { status: "error", kind: failure.kind, message: chain.next().unwrap_or_default(), causes: chain.collect() }
    }
}

/// Prints the report (or the failure's report) to stdout and returns the
/// exit code: 0, 2 for input failures, 1 for internal ones.
pub fn finish<T: Serialize>(result: Result<T, Failure>, format: Format) -> ExitCode {
    let (rendered, code) = match &result {
        Ok(report) => (render(report, format), ExitCode::SUCCESS),
        Err(failure) => {
            if failure.kind == FailureKind::Internal {
                eprintln!("internal error: {:?}", failure.error);
            }
            (render(&FailureReport::from(failure), format), failure.kind.exit_code())
        }
    };
    match rendered {
        Ok(text) => {
            print!("{text}");
            code
        }
        Err(e) => {
            eprintln!("internal error: {e}");
            FailureKind::Internal.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Row {
        id: u32,
        crouch: bool,
    }

    #[derive(Serialize)]
    struct Sample {
        status: &'static str,
        rows: Vec<Row>,
    }

    fn sample() -> Sample {
        Sample { status: "ok", rows: vec![Row { id: 1, crouch: false }, Row { id: 2, crouch: true }] }
    }

    #[test]
    fn toon_renders_uniform_rows_as_a_table() {
        let text = render(&sample(), Format::Toon).expect("renders");
        assert_eq!(text, "status: ok\nrows[2]{id,crouch}:\n  1,false\n  2,true\n");
    }

    #[test]
    fn json_renders_pretty_with_a_trailing_newline() {
        let text = render(&sample(), Format::Json).expect("renders");
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(parsed["rows"][1]["crouch"], true);
        assert!(text.ends_with("}\n"));
        assert!(text.contains("\n  \"status\""), "indented");
    }

    #[test]
    fn a_failure_report_lists_its_causes_in_order() {
        let error = anyhow::anyhow!("lump points outside the file").context("parsing map.bsp");
        let report = FailureReport::from(&Failure::input(error));
        assert_eq!(report.status, "error");
        assert_eq!(report.kind, FailureKind::Input);
        assert_eq!(report.message, "parsing map.bsp");
        assert_eq!(report.causes, vec!["lump points outside the file".to_string()]);
    }

    #[test]
    fn input_and_internal_failures_exit_differently() {
        assert_eq!(FailureKind::Input.exit_code(), ExitCode::from(2));
        assert_eq!(FailureKind::Internal.exit_code(), ExitCode::from(1));
    }
}
