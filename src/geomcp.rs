//! GeoMCP client — JSON-RPC 2.0 envelope over a pluggable [`Transport`].
//!
//! The envelope semantics are contract, so this client checks them explicitly
//! rather than trusting the peer: `jsonrpc` must be exactly `"2.0"`, the
//! response `id` must echo the request `id`, and a response must carry exactly
//! one of `result` / `error`. The shared conformance vectors
//! (`contracts/conformance/vectors.json`) pin these down for every language.

use crate::error::{jsonrpc, Error, GeoMcpError};
use crate::json::Json;
use crate::transport::{HttpRequest, HttpResponse, Transport};

/// Methods every GeoMCP implementation must answer.
pub const METHODS: [&str; 6] = [
    "geo.capabilities",
    "geo.describe",
    "geo.execute",
    "geo.invoke",
    "geo.plan",
    "geo.health",
];

/// Methods that consume resources or change state (need an API key).
pub const WRITE_METHODS: [&str; 2] = ["geo.execute", "geo.invoke"];

/// Build the JSON-RPC request body for `method` / `params`.
pub fn build_request(method: &str, params: Json, id: &str) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string(method)),
        ("params", params),
        ("id", Json::string(id)),
    ])
}

/// A GeoMCP node client.
///
/// Generic over [`Transport`], so tests (and callers with their own HTTP stack)
/// never need a socket.
pub struct GeoMcpClient<'a, T: Transport> {
    base_url: String,
    api_key: Option<String>,
    transport: &'a T,
    counter: std::sync::atomic::AtomicU64,
}

impl<'a, T: Transport> GeoMcpClient<'a, T> {
    pub fn new(base_url: impl Into<String>, transport: &'a T) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: None,
            transport,
            counter: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    fn next_id(&self) -> String {
        let n = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("rs-{}-{}", std::process::id(), n)
    }

    /// `geo.capabilities` — what this node can do.
    pub fn capabilities(&self) -> Result<Json, Error> {
        self.call("geo.capabilities", Json::object(Vec::<(String, Json)>::new()))
    }

    /// `geo.health` — liveness and self-checks.
    pub fn health(&self) -> Result<Json, Error> {
        self.call("geo.health", Json::object(Vec::<(String, Json)>::new()))
    }

    /// `geo.describe` — contracts of specific cards/skills/recipes.
    pub fn describe(&self, params: Json) -> Result<Json, Error> {
        self.call("geo.describe", params)
    }

    /// `geo.execute` — run a skill.
    pub fn execute(&self, params: Json) -> Result<Json, Error> {
        self.call("geo.execute", params)
    }

    /// `geo.plan` — materialise a recipe + params into a task graph.
    pub fn plan(&self, params: Json) -> Result<Json, Error> {
        self.call("geo.plan", params)
    }

    /// POST one JSON-RPC request and enforce the envelope contract.
    pub fn call(&self, method: &str, params: Json) -> Result<Json, Error> {
        let id = self.next_id();
        let payload = build_request(method, params, &id);

        let mut request = HttpRequest::new("POST", format!("{}/geomcp", self.base_url))
            .header("content-type", "application/json")
            .body(payload.to_compact_string());
        if let Some(key) = &self.api_key {
            request = request.header("x-api-key", key);
        }

        let response = self
            .transport
            .request(&request)
            .map_err(|e| Error::InvalidArgument(format!("GeoMCP transport error: {}", e)))?;

        parse_envelope(&response, &id)
    }
}

/// Parse and verify a GeoMCP response envelope.
///
/// Split out so the envelope rules are testable without any transport at all.
pub fn parse_envelope(response: &HttpResponse, expected_id: &str) -> Result<Json, Error> {
    let body = Json::parse(&response.body).map_err(|_| {
        Error::InvalidArgument(format!(
            "GeoMCP response is not JSON (status={})",
            response.status
        ))
    })?;

    if body.as_object().is_none() {
        return Err(Error::InvalidArgument(format!(
            "Invalid JSON-RPC response envelope (status={})",
            response.status
        )));
    }
    if body.get_str("jsonrpc") != Some("2.0") {
        return Err(Error::InvalidArgument(format!(
            "Invalid JSON-RPC response envelope (status={})",
            response.status
        )));
    }

    let id_matches = match body.get("id") {
        Some(Json::String(s)) => s == expected_id,
        _ => false,
    };
    if !id_matches {
        return Err(Error::InvalidArgument(format!(
            "JSON-RPC id mismatch: expected {:?}, got {}",
            expected_id,
            body.get("id").map(|v| v.to_string()).unwrap_or_default()
        )));
    }

    if let Some(error) = body.get("error") {
        if error != &Json::Null {
            let code = error.get("code").and_then(Json::as_i64).unwrap_or(jsonrpc::INTERNAL_ERROR);
            let message = error
                .get_str("message")
                .unwrap_or("GeoMCP error")
                .to_string();
            return Err(Error::GeoMcp(match error.get("data") {
                Some(data) if data != &Json::Null => GeoMcpError::with_data(code, message, data.clone()),
                _ => GeoMcpError::new(code, message),
            }));
        }
    }

    match body.get("result") {
        None => Err(Error::InvalidArgument(
            "JSON-RPC response has neither result nor error".to_string(),
        )),
        Some(result) if result.as_object().is_some() => Ok(result.clone()),
        Some(_) => Err(Error::InvalidArgument(
            "GeoMCP result must be an object".to_string(),
        )),
    }
}

/// List the skill names a node advertises.
///
/// GeoMCP puts native skills in `skills`; `tools` is for imported MCP tools.
/// Reading the wrong one silently yields an empty list.
pub fn advertised_skills(capabilities: &Json) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(items) = capabilities.get("skills").and_then(Json::as_array) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Transport that echoes the request id, so tests never depend on how the
    /// client generates ids (pre-computing them caused an off-by-one).
    #[derive(Default)]
    struct EchoTransport {
        seen: std::sync::Mutex<Vec<HttpRequest>>,
    }

    impl EchoTransport {
        fn last(&self) -> Option<HttpRequest> {
            self.seen.lock().unwrap().last().cloned()
        }
    }

    impl Transport for EchoTransport {
        fn request(&self, request: &HttpRequest) -> Result<HttpResponse, crate::error::HttpError> {
            self.seen.lock().unwrap().push(request.clone());
            let body = Json::parse(request.body.as_deref().unwrap_or("{}")).ok();
            let id = body
                .as_ref()
                .and_then(|b| b.get_str("id"))
                .unwrap_or("")
                .to_string();
            Ok(HttpResponse::new(
                200,
                format!(r#"{{"jsonrpc":"2.0","id":"{}","result":{{}}}}"#, id),
            ))
        }
    }

    fn envelope(payload: &str, id: &str) -> HttpResponse {
        let with_id = payload.replace("\"id\":\"__ID__\"", &format!("\"id\":\"{}\"", id));
        HttpResponse::new(200, with_id)
    }

    #[test]
    fn method_tables_are_consistent() {
        for method in WRITE_METHODS {
            assert!(METHODS.contains(&method), "{} must be a known method", method);
        }
        for readonly in ["geo.capabilities", "geo.describe", "geo.health", "geo.plan"] {
            assert!(!WRITE_METHODS.contains(&readonly), "{} must be read-only", readonly);
        }
    }

    #[test]
    fn error_code_ranges_are_disjoint() {
        for code in [
            jsonrpc::PARSE_ERROR,
            jsonrpc::INVALID_REQUEST,
            jsonrpc::METHOD_NOT_FOUND,
            jsonrpc::INVALID_PARAMS,
            jsonrpc::INTERNAL_ERROR,
        ] {
            assert!(code < 0, "reserved code {} must be negative", code);
        }
        for code in [
            crate::error::app::CONTRACT_NOT_SATISFIED,
            crate::error::app::EXECUTION_FAILED,
            crate::error::app::FEDERATION_UNRESOLVED,
        ] {
            assert!((2000..3000).contains(&code), "app code {} must be 2xxx", code);
        }
    }

    #[test]
    fn parses_a_successful_envelope() {
        let response = envelope(r#"{"jsonrpc":"2.0","id":"__ID__","result":{"ok":true}}"#, "id-1");
        let result = parse_envelope(&response, "id-1").unwrap();
        assert_eq!(result.get("ok").and_then(Json::as_bool), Some(true));
    }

    #[test]
    fn surfaces_application_errors_with_their_code() {
        let response = envelope(
            r#"{"jsonrpc":"2.0","id":"__ID__","error":{"code":2001,"message":"no skill"}}"#,
            "id-1",
        );
        match parse_envelope(&response, "id-1") {
            Err(Error::GeoMcp(err)) => {
                assert_eq!(err.code, 2001);
                assert_eq!(err.category(), "skill-not-found");
                assert!(!err.is_protocol_level());
            }
            other => panic!("expected GeoMcpError, got {:?}", other),
        }
    }

    #[test]
    fn rejects_a_mismatched_id() {
        let response = HttpResponse::new(
            200,
            r#"{"jsonrpc":"2.0","id":"someone-else","result":{}}"#,
        );
        let message = parse_envelope(&response, "id-1").unwrap_err().to_string();
        assert!(message.contains("id mismatch"), "got: {}", message);
    }

    #[test]
    fn rejects_a_non_2_0_envelope() {
        let response = HttpResponse::new(200, r#"{"jsonrpc":"1.0","id":"id-1","result":{}}"#);
        assert!(parse_envelope(&response, "id-1").is_err());
    }

    #[test]
    fn rejects_a_response_without_result_or_error() {
        let response = HttpResponse::new(200, r#"{"jsonrpc":"2.0","id":"id-1"}"#);
        let message = parse_envelope(&response, "id-1").unwrap_err().to_string();
        assert!(message.contains("neither result nor error"), "got: {}", message);
    }

    #[test]
    fn rejects_a_non_object_result() {
        let response = HttpResponse::new(200, r#"{"jsonrpc":"2.0","id":"id-1","result":[1,2]}"#);
        let message = parse_envelope(&response, "id-1").unwrap_err().to_string();
        assert!(message.contains("must be an object"), "got: {}", message);
    }

    #[test]
    fn rejects_non_json_bodies() {
        let response = HttpResponse::new(200, "<html>oops</html>");
        let message = parse_envelope(&response, "id-1").unwrap_err().to_string();
        assert!(message.contains("not JSON"), "got: {}", message);
    }

    #[test]
    fn client_posts_to_geomcp_with_jsonrpc_body() {
        let transport = EchoTransport::default();
        let client = GeoMcpClient::new("http://node.test/", &transport);
        client.health().unwrap();

        let request = transport.last().unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "http://node.test/geomcp");
        let body = Json::parse(request.body.as_deref().unwrap()).unwrap();
        assert_eq!(body.get_str("jsonrpc"), Some("2.0"));
        assert_eq!(body.get_str("method"), Some("geo.health"));
    }

    #[test]
    fn api_key_is_sent_only_when_configured() {
        let transport = EchoTransport::default();
        let keyed = GeoMcpClient::new("http://node.test", &transport).with_api_key("secret");
        keyed.health().unwrap();
        assert_eq!(
            transport.last().unwrap().headers.get("x-api-key"),
            Some(&"secret".to_string())
        );

        let plain = GeoMcpClient::new("http://node.test", &transport);
        plain.health().unwrap();
        assert!(!transport
            .last()
            .unwrap()
            .headers
            .contains_key("x-api-key"));
    }

    #[test]
    fn advertised_skills_handles_both_forms() {
        let caps = Json::object([(
            "skills",
            Json::Array(vec![
                Json::string("a"),
                Json::object([("name", Json::string("b"))]),
            ]),
        )]);
        assert_eq!(advertised_skills(&caps), vec!["a".to_string(), "b".to_string()]);
        assert!(advertised_skills(&Json::object(Vec::<(String, Json)>::new())).is_empty());
    }
}
