//! The entity lump: `{ "key" "value" ... }` blocks of quoted strings.

use crate::core::geom::Vec3;
use crate::core::world::Entity;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EntityError {
    #[error("entity lump, byte {at}: {problem}")]
    Syntax { at: usize, problem: &'static str },
    #[error("entity {index} ({classname}): origin {value:?} is not three numbers")]
    BadOrigin { index: usize, classname: String, value: String },
}

/// Parses the whole entity lump. Missing `origin` means the world origin.
pub fn parse_entities(text: &str) -> Result<Vec<Entity>, EntityError> {
    let mut tokens = Tokens { text: text.as_bytes(), at: 0 };
    let mut entities = Vec::new();
    while let Some(token) = tokens.next()? {
        if token != Token::Open {
            return Err(tokens.error("expected '{' to start an entity"));
        }
        let mut keys = Vec::new();
        loop {
            match tokens.next()? {
                Some(Token::Close) => break,
                Some(Token::Quoted(key)) => match tokens.next()? {
                    Some(Token::Quoted(value)) => keys.push((key, value)),
                    _ => return Err(tokens.error("key without a value")),
                },
                Some(Token::Open) => return Err(tokens.error("'{' inside an entity")),
                None => return Err(tokens.error("entity is never closed")),
            }
        }
        entities.push(entity_from_keys(entities.len(), keys)?);
    }
    Ok(entities)
}

fn entity_from_keys(index: usize, keys: Vec<(String, String)>) -> Result<Entity, EntityError> {
    let lookup = |name: &str| keys.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    let classname = lookup("classname").unwrap_or_default().to_string();
    let origin = match lookup("origin") {
        None => Vec3::ZERO,
        Some(value) => parse_origin(value).ok_or_else(|| EntityError::BadOrigin {
            index,
            classname: classname.clone(),
            value: value.to_string(),
        })?,
    };
    let brush_model = lookup("model").and_then(|m| m.strip_prefix('*')).and_then(|n| n.parse().ok());
    Ok(Entity::new(classname, origin, brush_model, keys))
}

fn parse_origin(value: &str) -> Option<Vec3> {
    let parts: Vec<f32> = value.split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?;
    match parts[..] {
        [x, y, z] => Some(Vec3::new(x, y, z)),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Open,
    Close,
    Quoted(String),
}

struct Tokens<'a> {
    text: &'a [u8],
    at: usize,
}

impl Tokens<'_> {
    fn error(&self, problem: &'static str) -> EntityError {
        EntityError::Syntax { at: self.at, problem }
    }

    /// The next token, or `None` at the end; trailing NULs count as space.
    fn next(&mut self) -> Result<Option<Token>, EntityError> {
        while self.at < self.text.len() && (self.text[self.at].is_ascii_whitespace() || self.text[self.at] == 0) {
            self.at += 1;
        }
        let Some(&byte) = self.text.get(self.at) else {
            return Ok(None);
        };
        self.at += 1;
        match byte {
            b'{' => Ok(Some(Token::Open)),
            b'}' => Ok(Some(Token::Close)),
            b'"' => {
                let start = self.at;
                let len = self.text[start..]
                    .iter()
                    .position(|b| *b == b'"')
                    .ok_or_else(|| self.error("unterminated string"))?;
                self.at = start + len + 1;
                Ok(Some(Token::Quoted(String::from_utf8_lossy(&self.text[start..start + len]).into_owned())))
            }
            _ => Err(self.error("unexpected character")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_parse_with_typed_keys() {
        let text = "{\n\"classname\" \"worldspawn\"\n}\n{\n\"origin\" \"1 -2 3.5\"\n\"classname\" \"info_player_teamspawn\"\n\"TeamNum\" \"2\"\n}\n{\n\"model\" \"*3\"\n\"classname\" \"func_brush\"\n}\n\0";
        let entities = parse_entities(text).expect("well formed");
        assert_eq!(entities.len(), 3);
        assert_eq!(entities[0].classname, "worldspawn");
        assert_eq!(entities[0].origin, Vec3::ZERO);
        assert_eq!(entities[1].origin, Vec3::new(1.0, -2.0, 3.5));
        assert_eq!(entities[1].get("TeamNum"), Some("2"));
        assert_eq!(entities[2].brush_model, Some(3));
        assert_eq!(entities[1].brush_model, None);
    }

    #[test]
    fn a_studio_model_is_not_a_brush_model() {
        let entities = parse_entities("{ \"classname\" \"prop_dynamic\" \"model\" \"models/a.mdl\" }").expect("ok");
        assert_eq!(entities[0].brush_model, None);
    }

    #[test]
    fn an_unclosed_entity_is_a_syntax_error() {
        assert!(matches!(parse_entities("{ \"classname\" \"x\""), Err(EntityError::Syntax { .. })));
    }

    #[test]
    fn a_key_without_a_value_is_a_syntax_error() {
        assert!(matches!(parse_entities("{ \"classname\" }"), Err(EntityError::Syntax { .. })));
    }

    #[test]
    fn a_malformed_origin_is_refused() {
        let result = parse_entities("{ \"classname\" \"info_player_start\" \"origin\" \"1 2\" }");
        assert!(matches!(result, Err(EntityError::BadOrigin { index: 0, .. })));
    }
}
