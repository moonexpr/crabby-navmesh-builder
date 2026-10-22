//! Use cases: each coordinates the seams for one thing a tool does, and
//! sorts what goes wrong into the caller's fault or ours.

pub mod analyze;
pub mod content;
pub mod generate;
pub mod inspect;
pub mod locate;
pub mod models;
pub mod report;
pub mod system;

use serde::Serialize;

/// Who has to act on a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The input was missing, unreadable, malformed, or unsupported: fix the input.
    Input,
    /// Something went wrong in the tool or the machine: report it.
    Internal,
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

/// A positive, finite distance in world units, parsed from an argument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Length(f32);

impl Length {
    pub fn get(self) -> f32 {
        self.0
    }
}

impl std::str::FromStr for Length {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let value: f32 = text.parse().map_err(|e| format!("{text:?} is not a number: {e}"))?;
        if value.is_finite() && value > 0.0 {
            Ok(Length(value))
        } else {
            Err(format!("{text:?} must be a positive distance"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_must_be_positive_and_finite() {
        assert_eq!("25".parse::<Length>().map(Length::get), Ok(25.0));
        for bad in ["0", "-3", "inf", "NaN", "far"] {
            assert!(bad.parse::<Length>().is_err(), "{bad}");
        }
    }
}
