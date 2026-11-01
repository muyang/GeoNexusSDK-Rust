//! GeoCard Registry client — discovery, registration and review.
//!
//! Two rules from the Python/TypeScript SDKs carry over verbatim because they
//! trip people up:
//!
//!   1. Registration is not publication. On a registry opened for public
//!      submissions every write lands as `pending` and stays invisible to
//!      `search()` until a *different* principal approves it.
//!   2. A card is findable by capability only if it declares `capabilities`.
//!      Tags do not count.

use crate::error::{Error, RegistryError};
use crate::json::Json;
use crate::transport::{HttpRequest, Transport};

pub struct RegistryClient<'a, T: Transport> {
    base_url: String,
    api_key: Option<String>,
    transport: &'a T,
}

impl<'a, T: Transport> RegistryClient<'a, T> {
    pub fn new(base_url: impl Into<String>, transport: &'a T) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: None,
            transport,
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Register a card owned by `node_url`.
    pub fn register(&self, card: Json, node_url: &str) -> Result<Json, Error> {
        let payload = Json::object([
            ("card", card),
            ("node_url", Json::string(node_url)),
        ]);
        self.call("POST", "/cards", Some(payload))
    }

    /// Register with an explicit review status (`"pending"` for review queues).
    pub fn register_with_status(
        &self,
        card: Json,
        node_url: &str,
        status: &str,
    ) -> Result<Json, Error> {
        let payload = Json::object([
            ("card", card),
            ("node_url", Json::string(node_url)),
            ("status", Json::string(status)),
        ]);
        self.call("POST", "/cards", Some(payload))
    }

    pub fn unregister(&self, card_id: &str) -> Result<Json, Error> {
        self.call("DELETE", &format!("/cards/{}", card_id), None)
    }

    pub fn get(&self, card_id: &str) -> Result<Json, Error> {
        self.call("GET", &format!("/cards/{}", card_id), None)
    }

    /// Approve a pending submission. Needs the reviewer's key.
    pub fn approve(&self, card_id: &str, note: &str) -> Result<Json, Error> {
        let payload = Json::object([("note", Json::string(note))]);
        self.call("POST", &format!("/cards/{}/approve", card_id), Some(payload))
    }

    pub fn reject(&self, card_id: &str, note: &str) -> Result<Json, Error> {
        let payload = Json::object([("note", Json::string(note))]);
        self.call("POST", &format!("/cards/{}/reject", card_id), Some(payload))
    }

    /// Nodes known to this registry.
    pub fn nodes(&self) -> Result<Json, Error> {
        self.call("GET", "/nodes", None)
    }

    pub fn health(&self) -> Result<Json, Error> {
        self.call("GET", "/health", None)
    }

    /// Contract-aware search. Returns the raw `results` array entries.
    ///
    /// `query` maps directly to query-string parameters, e.g.
    /// `vec![("capability", "ndvi"), ("bbox", "100,20,110,30")]`.
    pub fn search(&self, query: &[(&str, &str)]) -> Result<Vec<Json>, Error> {
        let mut path = String::from("/search");
        if !query.is_empty() {
            let pairs: Vec<String> = query
                .iter()
                .map(|(k, v)| format!("{}={}", k, urlencode(v)))
                .collect();
            path.push('?');
            path.push_str(&pairs.join("&"));
        }
        let body = self.call("GET", &path, None)?;
        Ok(body
            .get("results")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default())
    }

    fn call(&self, method: &str, path: &str, body: Option<Json>) -> Result<Json, Error> {
        let mut request = HttpRequest::new(method, format!("{}{}", self.base_url, path));
        if let Some(key) = &self.api_key {
            request = request.header("x-api-key", key);
        }
        if let Some(payload) = body {
            request = request
                .header("content-type", "application/json")
                .body(payload.to_compact_string());
        }

        let response = self
            .transport
            .request(&request)
            .map_err(|e| Error::InvalidArgument(format!("Registry transport error: {}", e)))?;

        if !response.is_success() {
            return Err(Error::Registry(RegistryError::with_status(
                truncate(&response.body, 300),
                response.status,
                truncate(&response.body, 1000),
            )));
        }

        Json::parse(&response.body).map_err(|e| {
            Error::Registry(RegistryError::with_status(
                format!("Registry returned non-JSON: {}", e),
                response.status,
                truncate(&response.body, 300),
            ))
        })
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "…"
}

/// Minimal percent-encoding for query values.
///
/// Unreserved characters (letters, digits, `-._~`) pass through; everything
/// else is percent-encoded byte-wise, which is correct for UTF-8 input.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let c = *byte as char;
        if c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '~' {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn register_posts_card_and_node_url() {
        let transport = MockTransport::new();
        transport.push(200, r#"{"status":"registered"}"#);
        let client = RegistryClient::new("http://reg.test", &transport);
        let card = Json::object([("id", Json::string("a"))]);
        client.register(card, "http://node:8787").unwrap();

        let request = transport.last_request().unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "http://reg.test/cards");
        let body = Json::parse(request.body.as_deref().unwrap()).unwrap();
        assert_eq!(body.get_str("node_url"), Some("http://node:8787"));
        assert_eq!(
            body.get("card").and_then(|c| c.get_str("id")),
            Some("a")
        );
    }

    #[test]
    fn search_builds_a_query_string() {
        let transport = MockTransport::new();
        transport.push(200, r#"{"results":[]}"#);
        let client = RegistryClient::new("http://reg.test", &transport);
        client
            .search(&[("capability", "ndvi"), ("bbox", "100,20,110,30")])
            .unwrap();
        let url = transport.last_request().unwrap().url;
        assert!(url.starts_with("http://reg.test/search?"), "got {}", url);
        assert!(url.contains("capability=ndvi"), "got {}", url);
        assert!(url.contains("bbox=100%2C20%2C110%2C30"), "got {}", url);
    }

    #[test]
    fn surfaces_http_errors_with_status() {
        let transport = MockTransport::new();
        transport.push(422, r#"{"detail":"nope"}"#);
        let client = RegistryClient::new("http://reg.test", &transport);
        match client.nodes() {
            Err(Error::Registry(err)) => {
                assert_eq!(err.status, Some(422));
            }
            other => panic!("expected RegistryError, got {:?}", other),
        }
    }

    #[test]
    fn api_key_is_sent_only_when_configured() {
        let transport = MockTransport::new();
        transport.push(200, "{}");
        let client = RegistryClient::new("http://reg.test", &transport).with_api_key("k");
        client.health().unwrap();
        assert_eq!(
            transport.last_request().unwrap().headers.get("x-api-key"),
            Some(&"k".to_string())
        );
    }

    #[test]
    fn urlencode_handles_cjk_and_reserved_characters() {
        assert_eq!(urlencode("ndvi"), "ndvi");
        assert_eq!(urlencode("a b"), "a%20b");
        assert_eq!(urlencode("100,20"), "100%2C20");
        // UTF-8 bytes, not code points.
        assert_eq!(urlencode("蒙"), "%E8%92%99");
    }
}
