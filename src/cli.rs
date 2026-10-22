//! What both command-line tools share: the output format flag, and failures
//! reported in that format with an exit code saying whose fault they were.

use std::process::ExitCode;

use serde::Serialize;

use crate::report::{Format, render};

/// Who has to act on a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The input was missing, unreadable, or malformed: fix the input.
    Input,
    /// Something went wrong in the tool or the machine: report it.
    Internal,
}

impl FailureKind {
    pub fn exit_code(self) -> ExitCode {
        match self {
            FailureKind::Input => ExitCode::from(2),
            FailureKind::Internal => ExitCode::from(1),
        }
    }
}

#[derive(Debug)]
pub struct Failure {
    pub kind: FailureKind,
    pub error: anyhow::Error,
}

impl Failure {
    pub fn input(error: impl Into<anyhow::Error>) -> Self {
        Self { kind: FailureKind::Input, error: error.into() }
    }

    pub fn internal(error: impl Into<anyhow::Error>) -> Self {
        Self { kind: FailureKind::Internal, error: error.into() }
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
