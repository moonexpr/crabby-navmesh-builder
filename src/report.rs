//! Machine-readable output for the command-line tools: TOON by default,
//! JSON on request. Both render the same serializable report values.

use serde::Serialize;

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
}
