//! A minimal JSON implementation (parser + serializer).
//!
//! Hand-rolled because this crate has no dependencies (see `Cargo.toml`).
//!
//! Scope is the JSON value space as used by GeoCard / GeoMCP / OGE:
//! objects, arrays, strings, numbers, booleans, null. Numbers are kept as
//! `f64` and rendered without trailing `.0` when they are integral, so
//! `4` round-trips as `4` and `4.5` as `4.5` — registries compare these.
//!
//! Deliberate choices:
//!   * Object keys keep insertion order (stable output, easier diffs).
//!   * Duplicate keys: last one wins, matching most JSON parsers.
//!   * Numbers that are not representable as `f64` are rejected rather than
//!     silently mangled — see [`JsonError::NumberOutOfRange`].

use std::collections::BTreeMap;
use std::fmt;

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    /// Convenience constructor for an object from `(key, value)` pairs.
    pub fn object<I, K>(pairs: I) -> Json
    where
        I: IntoIterator<Item = (K, Json)>,
        K: Into<String>,
    {
        Json::Object(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    pub fn string(value: impl Into<String>) -> Json {
        Json::String(value.into())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Number(n) if n.fract() == 0.0 => Some(*n as i64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, Json>> {
        match self {
            Json::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Look up a key in an object; `None` for non-objects.
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.as_object().and_then(|m| m.get(key))
    }

    /// Look up a string key.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Json::as_str)
    }

    /// Look up a key and return the first present string among `keys`.
    ///
    /// OGE is inconsistent about field names across endpoints, so call sites
    /// read several candidates rather than guessing one.
    pub fn pick_str(&self, keys: &[&str]) -> Option<String> {
        for key in keys {
            if let Some(value) = self.get(key) {
                if let Some(s) = value.as_str() {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
                if let Some(n) = value.as_i64() {
                    return Some(n.to_string());
                }
            }
        }
        None
    }

    /// Insert into an object; panics for non-objects (callers build objects).
    pub fn insert(&mut self, key: impl Into<String>, value: Json) {
        match self {
            Json::Object(map) => {
                map.insert(key.into(), value);
            }
            _ => panic!("Json::insert called on a non-object"),
        }
    }

    /// Insert only when `value` is `Some` — keeps "unknown" fields absent
    /// instead of emitting `null`, which registries treat differently.
    pub fn insert_opt(&mut self, key: impl Into<String>, value: Option<Json>) {
        if let Some(value) = value {
            self.insert(key, value);
        }
    }

    /// Serialize to compact JSON text.
    ///
    /// Kept as an explicit method so call sites read clearly; `Display` also
    /// produces compact JSON, so `format!("{}", value)` is equivalent.
    pub fn to_compact_string(&self) -> String {
        let mut out = String::new();
        self.write_to(&mut out);
        out
    }

    /// Serialize with two-space indentation (readable cards/diffs).
    pub fn to_string_pretty(&self) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out
    }

    fn write_to(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Number(n) => out.push_str(&format_number(*n)),
            Json::String(s) => write_string(out, s),
            Json::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_to(out);
                }
                out.push(']');
            }
            Json::Object(map) => {
                out.push('{');
                for (i, (key, value)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(out, key);
                    out.push(':');
                    value.write_to(out);
                }
                out.push('}');
            }
        }
    }

    fn write_pretty(&self, out: &mut String, depth: usize) {
        let pad = |d: usize| "  ".repeat(d);
        match self {
            Json::Array(items) if !items.is_empty() => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    out.push_str(&pad(depth + 1));
                    item.write_pretty(out, depth + 1);
                }
                out.push('\n');
                out.push_str(&pad(depth));
                out.push(']');
            }
            Json::Object(map) if !map.is_empty() => {
                out.push_str("{\n");
                for (i, (key, value)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    out.push_str(&pad(depth + 1));
                    write_string(out, key);
                    out.push_str(": ");
                    value.write_pretty(out, depth + 1);
                }
                out.push('\n');
                out.push_str(&pad(depth));
                out.push('}');
            }
            other => other.write_to(out),
        }
    }

    /// Parse JSON text.
    pub fn parse(text: &str) -> Result<Json, JsonError> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            pos: 0,
        };
        let value = parser.parse_value()?;
        parser.skip_ws();
        if parser.pos != parser.bytes.len() {
            return Err(JsonError::TrailingCharacters(parser.pos));
        }
        Ok(value)
    }
}

impl std::fmt::Display for Json {
    /// Compact JSON, same bytes as [`Json::to_compact_string`].
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_compact_string())
    }
}

/// Errors produced while parsing JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonError {
    UnexpectedEnd,
    UnexpectedByte(u8, usize),
    InvalidEscape(usize),
    InvalidUnicode(usize),
    TrailingComma(usize),
    TrailingCharacters(usize),
    NumberOutOfRange(usize),
    ControlCharacterInString(usize),
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::UnexpectedEnd => write!(f, "unexpected end of JSON input"),
            JsonError::UnexpectedByte(b, pos) => {
                write!(f, "unexpected byte {:?} at offset {}", *b as char, pos)
            }
            JsonError::InvalidEscape(pos) => write!(f, "invalid escape at offset {}", pos),
            JsonError::InvalidUnicode(pos) => write!(f, "invalid unicode escape at offset {}", pos),
            JsonError::TrailingComma(pos) => write!(f, "trailing comma at offset {}", pos),
            JsonError::TrailingCharacters(pos) => write!(f, "trailing characters at offset {}", pos),
            JsonError::NumberOutOfRange(pos) => write!(f, "number out of range at offset {}", pos),
            JsonError::ControlCharacterInString(pos) => {
                write!(f, "raw control character in string at offset {}", pos)
            }
        }
    }
}

impl std::error::Error for JsonError {}

/// Render a number the way JSON expects: no trailing `.0` for integers.
fn format_number(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else if n.is_finite() {
        // `{}` for f64 already produces the shortest round-trip form.
        format!("{}", n)
    } else {
        // JSON has no NaN/Infinity. Emitting `null` keeps the document valid
        // rather than producing something no parser will accept.
        "null".to_string()
    }
}

fn write_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> Result<Json, JsonError> {
        self.skip_ws();
        match self.peek() {
            None => Err(JsonError::UnexpectedEnd),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => Ok(Json::String(self.parse_string()?)),
            Some(b't') => self.expect_literal("true", Json::Bool(true)),
            Some(b'f') => self.expect_literal("false", Json::Bool(false)),
            Some(b'n') => self.expect_literal("null", Json::Null),
            Some(b'-') | Some(b'0'..=b'9') => self.parse_number(),
            Some(b) => Err(JsonError::UnexpectedByte(b, self.pos)),
        }
    }

    fn expect_literal(&mut self, literal: &str, value: Json) -> Result<Json, JsonError> {
        let end = self.pos + literal.len();
        if end > self.bytes.len() || &self.bytes[self.pos..end] != literal.as_bytes() {
            return Err(JsonError::UnexpectedByte(self.bytes[self.pos], self.pos));
        }
        self.pos = end;
        Ok(value)
    }

    fn parse_object(&mut self) -> Result<Json, JsonError> {
        self.pos += 1; // consume '{'
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(match self.peek() {
                    Some(b) => JsonError::UnexpectedByte(b, self.pos),
                    None => JsonError::UnexpectedEnd,
                });
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(match self.peek() {
                    Some(b) => JsonError::UnexpectedByte(b, self.pos),
                    None => JsonError::UnexpectedEnd,
                });
            }
            self.pos += 1;
            let value = self.parse_value()?;
            map.insert(key, value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_ws();
                    if self.peek() == Some(b'}') {
                        return Err(JsonError::TrailingComma(self.pos));
                    }
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Object(map));
                }
                Some(b) => return Err(JsonError::UnexpectedByte(b, self.pos)),
                None => return Err(JsonError::UnexpectedEnd),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Json, JsonError> {
        self.pos += 1; // consume '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            let value = self.parse_value()?;
            items.push(value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_ws();
                    if self.peek() == Some(b']') {
                        return Err(JsonError::TrailingComma(self.pos));
                    }
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                Some(b) => return Err(JsonError::UnexpectedByte(b, self.pos)),
                None => return Err(JsonError::UnexpectedEnd),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, JsonError> {
        self.pos += 1; // consume opening quote
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(JsonError::UnexpectedEnd),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    let escape_at = self.pos;
                    self.pos += 1;
                    match self.peek() {
                        None => return Err(JsonError::UnexpectedEnd),
                        Some(b'"') => {
                            out.push('"');
                            self.pos += 1;
                        }
                        Some(b'\\') => {
                            out.push('\\');
                            self.pos += 1;
                        }
                        Some(b'/') => {
                            out.push('/');
                            self.pos += 1;
                        }
                        Some(b'b') => {
                            out.push('\u{08}');
                            self.pos += 1;
                        }
                        Some(b'f') => {
                            out.push('\u{0c}');
                            self.pos += 1;
                        }
                        Some(b'n') => {
                            out.push('\n');
                            self.pos += 1;
                        }
                        Some(b'r') => {
                            out.push('\r');
                            self.pos += 1;
                        }
                        Some(b't') => {
                            out.push('\t');
                            self.pos += 1;
                        }
                        Some(b'u') => {
                            self.pos += 1;
                            let cp = self.parse_hex4()?;
                            if (0xD800..0xDC00).contains(&cp) {
                                // High surrogate: a low surrogate must follow.
                                if self.peek() != Some(b'\\') {
                                    return Err(JsonError::InvalidUnicode(escape_at));
                                }
                                self.pos += 1;
                                if self.peek() != Some(b'u') {
                                    return Err(JsonError::InvalidUnicode(escape_at));
                                }
                                self.pos += 1;
                                let low = self.parse_hex4()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return Err(JsonError::InvalidUnicode(escape_at));
                                }
                                let combined =
                                    0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                                match char::from_u32(combined) {
                                    Some(ch) => out.push(ch),
                                    None => return Err(JsonError::InvalidUnicode(escape_at)),
                                }
                            } else if (0xDC00..0xE000).contains(&cp) {
                                return Err(JsonError::InvalidUnicode(escape_at));
                            } else {
                                match char::from_u32(cp) {
                                    Some(ch) => out.push(ch),
                                    None => return Err(JsonError::InvalidUnicode(escape_at)),
                                }
                            }
                        }
                        Some(_) => return Err(JsonError::InvalidEscape(escape_at)),
                    }
                }
                Some(b) if b < 0x20 => {
                    return Err(JsonError::ControlCharacterInString(self.pos));
                }
                Some(_) => {
                    // Decode one UTF-8 scalar starting here.
                    let rest = &self.bytes[self.pos..];
                    let text = std::str::from_utf8(rest)
                        .map_err(|_| JsonError::InvalidUnicode(self.pos))?;
                    let ch = text.chars().next().ok_or(JsonError::UnexpectedEnd)?;
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    fn parse_hex4(&mut self) -> Result<u32, JsonError> {
        let start = self.pos;
        if self.pos + 4 > self.bytes.len() {
            return Err(JsonError::UnexpectedEnd);
        }
        let mut value = 0u32;
        for i in 0..4 {
            let b = self.bytes[self.pos + i];
            let digit = match b {
                b'0'..=b'9' => (b - b'0') as u32,
                b'a'..=b'f' => (b - b'a' + 10) as u32,
                b'A'..=b'F' => (b - b'A' + 10) as u32,
                _ => return Err(JsonError::InvalidUnicode(start)),
            };
            value = value * 16 + digit;
        }
        self.pos += 4;
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() || b == b'.' || b == b'e' || b == b'E' || b == b'+' || b == b'-' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| JsonError::NumberOutOfRange(start))?;
        // Reject syntactically-invalid numbers that `f64::from_str` would accept
        // (e.g. "1e999" → inf, or leading '+').
        if text.is_empty() || text == "-" || text.contains("e") && text.ends_with('e') {
            return Err(JsonError::NumberOutOfRange(start));
        }
        let parsed: f64 = text
            .parse()
            .map_err(|_| JsonError::NumberOutOfRange(start))?;
        if !parsed.is_finite() {
            return Err(JsonError::NumberOutOfRange(start));
        }
        Ok(Json::Number(parsed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_objects_arrays_and_scalars() {
        let value = Json::parse(r#"{"a":1,"b":[true,false,null,"s"],"c":{"d":2.5}}"#).unwrap();
        assert_eq!(value.get("a").and_then(Json::as_i64), Some(1));
        assert_eq!(value.get("b").and_then(Json::as_array).map(|v| v.len()), Some(4));
        assert_eq!(
            value.get("c").and_then(|c| c.get("d")).and_then(Json::as_f64),
            Some(2.5)
        );
    }

    #[test]
    fn round_trips_integral_numbers_without_trailing_zero() {
        let parsed = Json::parse("4").unwrap();
        assert_eq!(parsed.to_compact_string(), "4");
        let parsed = Json::parse("4.5").unwrap();
        assert_eq!(parsed.to_compact_string(), "4.5");
    }

    #[test]
    fn decodes_escapes_including_surrogate_pairs() {
        let value = Json::parse(r#""a\u4e2d\ud83d\ude00\n""#).unwrap();
        assert_eq!(value.as_str().unwrap(), "a中😀\n");
    }

    #[test]
    fn keeps_cjk_as_utf8_not_escapes() {
        let value = Json::string("蒙古国界");
        assert_eq!(value.to_compact_string(), "\"蒙古国界\"");
    }

    #[test]
    fn rejects_trailing_commas_and_trailing_garbage() {
        assert!(matches!(
            Json::parse(r#"{"a":1,}"#),
            Err(JsonError::TrailingComma(_))
        ));
        assert!(matches!(
            Json::parse("[1,]"),
            Err(JsonError::TrailingComma(_))
        ));
        assert!(matches!(
            Json::parse("{} extra"),
            Err(JsonError::TrailingCharacters(_))
        ));
    }

    #[test]
    fn rejects_infinite_numbers_instead_of_mangling() {
        assert!(matches!(
            Json::parse("1e999"),
            Err(JsonError::NumberOutOfRange(_))
        ));
    }

    #[test]
    fn rejects_control_characters_inside_strings() {
        assert!(matches!(
            Json::parse("\"a\nb\""),
            Err(JsonError::ControlCharacterInString(_))
        ));
    }

    #[test]
    fn duplicate_keys_take_the_last_value() {
        let value = Json::parse(r#"{"a":1,"a":2}"#).unwrap();
        assert_eq!(value.get("a").and_then(Json::as_i64), Some(2));
    }

    #[test]
    fn pretty_output_is_stable() {
        let value = Json::parse(r#"{"b":1,"a":[1,2]}"#).unwrap();
        // BTreeMap ordering means keys sort deterministically.
        assert_eq!(
            value.to_string_pretty(),
            "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": 1\n}"
        );
    }

    #[test]
    fn pick_str_walks_candidate_keys() {
        let value = Json::parse(r#"{"x":null,"y":"","z":"hit"}"#).unwrap();
        assert_eq!(value.pick_str(&["x", "y", "z"]), Some("hit".to_string()));
        assert_eq!(value.pick_str(&["missing"]), None);
    }
}
