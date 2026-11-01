//! GeoCard: the unit of description in GeoNexus.
//!
//! Validation mirrors `schemas/geocard.schema.json`, which is embedded at
//! compile time via `include_str!`. A test parses that embedded schema with
//! this crate's own JSON parser and asserts the type enum here matches it, so
//! the Rust SDK cannot drift from the published contract (or from the
//! TypeScript/Python ports) without a test failing.
//!
//! Layering, mirrored from the Python SDK: [`parse_card`] is permissive and
//! [`validate_card`] is strict. Keeping them separate lets tooling inspect and
//! repair malformed cards instead of refusing to look at them.

use crate::error::GeoCardValidationError;
use crate::json::Json;

/// Asset kinds allowed by the published contract.
pub const GEOCARD_TYPES: [&str; 7] = [
    "data",
    "model",
    "skill",
    "agent",
    "workflow",
    "knowledge",
    "compute",
];

/// The published schema, embedded so the enum check cannot drift.
pub const GEOCARD_SCHEMA: &str = include_str!("../schemas/geocard.schema.json");

/// Required top-level fields.
pub const REQUIRED_FIELDS: [&str; 5] = ["geocard_version", "id", "type", "name", "description"];

/// True when `id` satisfies the contract pattern `^[A-Za-z0-9][A-Za-z0-9._-]*$`.
pub fn is_valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Every schema violation, as human-readable strings. Empty means valid.
///
/// This covers the constraints the published schema actually applies:
/// required fields, the type enum, the id pattern, and bbox arity/type.
/// It is intentionally not a general JSON-Schema engine — but unlike a
/// half-implemented engine, it does not silently ignore what it does not check:
/// [`validate_card`] is the only gate and it checks exactly the above.
pub fn card_errors(card: &Json) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(object) = card.as_object() else {
        return vec!["$: expected a GeoCard object".to_string()];
    };

    for field in REQUIRED_FIELDS {
        match object.get(field) {
            None => errors.push(format!("$: missing required \"{}\"", field)),
            Some(value) if value.as_str().is_none() => {
                errors.push(format!("$.{}: expected string", field))
            }
            _ => {}
        }
    }

    if let Some(id) = card.get_str("id") {
        if !is_valid_id(id) {
            errors.push(format!(
                "$.id: must match ^[A-Za-z0-9][A-Za-z0-9._-]*$, got {:?}",
                id
            ));
        }
    }

    if let Some(kind) = card.get_str("type") {
        if !GEOCARD_TYPES.contains(&kind) {
            errors.push(format!(
                "$.type: must be one of {:?}, got {:?}",
                GEOCARD_TYPES, kind
            ));
        }
    }

    if let Some(spatial) = card.get("spatial") {
        if let Some(bbox) = spatial.get("bbox") {
            match bbox.as_array() {
                None => errors.push("$.spatial.bbox: expected an array".to_string()),
                Some(items) => {
                    if items.len() < 4 || items.len() > 6 {
                        errors.push(format!(
                            "$.spatial.bbox: needs 4..6 numbers, got {}",
                            items.len()
                        ));
                    }
                    for (index, item) in items.iter().enumerate() {
                        if item.as_f64().is_none() {
                            errors.push(format!("$.spatial.bbox[{}]: expected number", index));
                        }
                    }
                }
            }
        }
    }

    if let Some(capabilities) = card.get("capabilities") {
        match capabilities.as_array() {
            None => errors.push("$.capabilities: expected an array".to_string()),
            Some(items) => {
                for (index, item) in items.iter().enumerate() {
                    let ok = item.as_str().is_some()
                        || item
                            .get("name")
                            .map(|name| name.as_str().is_some())
                            .unwrap_or(false);
                    if !ok {
                        errors.push(format!(
                            "$.capabilities[{}]: expected a string or {{\"name\": ...}}",
                            index
                        ));
                    }
                }
            }
        }
    }

    errors
}

/// Validate a card, returning it on success.
pub fn validate_card(card: Json) -> Result<Json, GeoCardValidationError> {
    let errors = card_errors(&card);
    if errors.is_empty() {
        Ok(card)
    } else {
        Err(GeoCardValidationError::new(errors))
    }
}

/// Parse without contract validation — for inspecting cards you did not write.
///
/// Still enforces that the required fields exist and are strings, because a
/// card without them cannot be reasoned about at all.
pub fn parse_card(card: Json) -> Result<Json, GeoCardValidationError> {
    let Some(object) = card.as_object() else {
        return Err(GeoCardValidationError::new(vec![
            "$: expected a GeoCard object".to_string(),
        ]));
    };
    let mut errors = Vec::new();
    for field in REQUIRED_FIELDS {
        if object.get(field).and_then(Json::as_str).is_none() {
            errors.push(format!("$: missing required string \"{}\"", field));
        }
    }
    if errors.is_empty() {
        Ok(card)
    } else {
        Err(GeoCardValidationError::new(errors))
    }
}

/// Names a card advertises, accepting both the string and object forms.
pub fn capability_names(card: &Json) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(items) = card.get("capabilities").and_then(Json::as_array) {
        for item in items {
            if let Some(name) = item.as_str() {
                names.push(name.to_string());
            } else if let Some(name) = item.get_str("name") {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Fluent builder, mirroring `GeoCardBuilder` in the Python/TypeScript SDKs.
#[derive(Debug, Clone)]
pub struct GeoCardBuilder {
    card: Json,
}

impl GeoCardBuilder {
    pub fn new(
        id: impl Into<String>,
        kind: &str,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let card = Json::object([
            ("geocard_version", Json::string("1.0")),
            ("id", Json::string(id.into())),
            ("type", Json::string(kind)),
            ("name", Json::string(name.into())),
            ("description", Json::string(description.into())),
            ("tags", Json::Array(Vec::new())),
        ]);
        Self { card }
    }

    pub fn spatial(mut self, bbox: Vec<f64>, crs: &str) -> Self {
        self.card.insert(
            "spatial",
            Json::object([
                (
                    "bbox",
                    Json::Array(bbox.into_iter().map(Json::Number).collect()),
                ),
                ("crs", Json::string(crs)),
            ]),
        );
        self
    }

    pub fn access(mut self, protocol: &str, endpoint: &str) -> Self {
        self.card.insert(
            "access",
            Json::object([
                ("protocol", Json::string(protocol)),
                ("endpoint", Json::string(endpoint)),
            ]),
        );
        self
    }

    pub fn capability(mut self, name: &str) -> Self {
        let mut items = self
            .card
            .get("capabilities")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default();
        items.push(Json::object([("name", Json::string(name))]));
        self.card.insert("capabilities", Json::Array(items));
        self
    }

    pub fn tag(mut self, tag: &str) -> Self {
        let mut items = self
            .card
            .get("tags")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default();
        let already = items.iter().any(|item| item.as_str() == Some(tag));
        if !already {
            items.push(Json::string(tag));
        }
        self.card.insert("tags", Json::Array(items));
        self
    }

    pub fn license(mut self, value: &str) -> Self {
        self.card.insert("license", Json::string(value));
        self
    }

    pub fn set(mut self, key: &str, value: Json) -> Self {
        self.card.insert(key, value);
        self
    }

    /// Build without validating — use when you still need to inspect gaps.
    pub fn build_unchecked(self) -> Json {
        self.card
    }

    /// Build and validate; fails if incomplete.
    pub fn build(self) -> Result<Json, GeoCardValidationError> {
        validate_card(self.card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> Json {
        Json::object([
            ("geocard_version", Json::string("1.0")),
            ("id", Json::string("asset-1")),
            ("type", Json::string("data")),
            ("name", Json::string("Asset")),
            ("description", Json::string("An asset.")),
        ])
    }

    #[test]
    fn embedded_schema_enum_matches_this_crate() {
        // Parsing the schema with our own parser also exercises json.rs on a
        // real 5 KB document.
        let schema = Json::parse(GEOCARD_SCHEMA).expect("bundled schema must parse");
        let enum_values = schema
            .get("properties")
            .and_then(|p| p.get("type"))
            .and_then(|t| t.get("enum"))
            .and_then(Json::as_array)
            .expect("schema must declare a type enum");
        let from_schema: Vec<String> = enum_values
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let mut a = from_schema.clone();
        let mut b: Vec<String> = GEOCARD_TYPES.iter().map(|s| s.to_string()).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "GEOCARD_TYPES drifted from schemas/geocard.schema.json");
    }

    #[test]
    fn accepts_a_minimal_card() {
        assert!(validate_card(minimal()).is_ok());
    }

    #[test]
    fn reports_every_missing_field() {
        let errors = card_errors(&Json::object([("geocard_version", Json::string("1.0"))]));
        for field in ["id", "type", "name", "description"] {
            assert!(
                errors.iter().any(|e| e.contains(field)),
                "expected an error mentioning {}: {:?}",
                field,
                errors
            );
        }
    }

    #[test]
    fn rejects_bad_ids() {
        assert!(is_valid_id("asset-1"));
        assert!(is_valid_id("a.b_c-d"));
        assert!(!is_valid_id("bad id"));
        assert!(!is_valid_id("a/b"));
        assert!(!is_valid_id("-leading"));
        assert!(!is_valid_id(""));
    }

    #[test]
    fn rejects_type_outside_the_enum() {
        let card = Json::object([
            ("geocard_version", Json::string("1.0")),
            ("id", Json::string("x.1")),
            ("type", Json::string("nope")),
            ("name", Json::string("n")),
            ("description", Json::string("d")),
        ]);
        let errors = card_errors(&card);
        assert!(errors.iter().any(|e| e.contains("must be one of")));
    }

    #[test]
    fn parse_is_permissive_about_the_enum() {
        // Mirrors Python's load_geocard(validate=False).
        let card = Json::object([
            ("geocard_version", Json::string("1.0")),
            ("id", Json::string("x.1")),
            ("type", Json::string("nope")),
            ("name", Json::string("n")),
            ("description", Json::string("d")),
        ]);
        assert!(parse_card(card).is_ok());
    }

    #[test]
    fn bbox_arity_is_enforced() {
        let ok = Json::object([
            ("geocard_version", Json::string("1.0")),
            ("id", Json::string("x.1")),
            ("type", Json::string("data")),
            ("name", Json::string("n")),
            ("description", Json::string("d")),
            (
                "spatial",
                Json::object([(
                    "bbox",
                    Json::Array(vec![
                        Json::Number(1.0),
                        Json::Number(2.0),
                        Json::Number(3.0),
                        Json::Number(4.0),
                    ]),
                )]),
            ),
        ]);
        assert!(card_errors(&ok).is_empty());

        let bad = Json::object([
            ("geocard_version", Json::string("1.0")),
            ("id", Json::string("x.1")),
            ("type", Json::string("data")),
            ("name", Json::string("n")),
            ("description", Json::string("d")),
            (
                "spatial",
                Json::object([(
                    "bbox",
                    Json::Array(vec![Json::Number(1.0), Json::Number(2.0)]),
                )]),
            ),
        ]);
        assert!(card_errors(&bad).iter().any(|e| e.contains("4..6")));
    }

    #[test]
    fn builder_produces_a_valid_card() {
        let card = GeoCardBuilder::new("b-1", "data", "Built", "desc")
            .spatial(vec![100.0, 20.0, 110.0, 30.0], "EPSG:4326")
            .capability("ndvi")
            .tag("demo")
            .build()
            .unwrap();
        assert_eq!(capability_names(&card), vec!["ndvi".to_string()]);
    }

    #[test]
    fn capability_names_handles_both_forms() {
        let card = Json::object([
            ("geocard_version", Json::string("1.0")),
            ("id", Json::string("x.1")),
            ("type", Json::string("data")),
            ("name", Json::string("n")),
            ("description", Json::string("d")),
            (
                "capabilities",
                Json::Array(vec![
                    Json::string("ndvi"),
                    Json::object([("name", Json::string("ndwi"))]),
                ]),
            ),
        ]);
        assert_eq!(
            capability_names(&card),
            vec!["ndvi".to_string(), "ndwi".to_string()]
        );
    }
}
