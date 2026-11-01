//! Pluggable HTTP transport.
//!
//! Two reasons this is a trait instead of a baked-in client:
//!
//!   1. **Testability.** GeoCard/GeoMCP behaviour can be exercised with
//!      [`MockTransport`] — no sockets, no flakiness, no network.
//!   2. **HTTPS.** `std` has no TLS, and this crate has no dependencies (see
//!      `Cargo.toml`). Rather than pretend, the default
//!      [`StdHttpTransport`] speaks plain HTTP only and rejects `https://` with
//!      an explicit error. Callers who need TLS can implement [`Transport`]
//!      over their own client — the crate's logic does not care.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::error::HttpError;

/// A single HTTP request.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    /// Absolute URL, e.g. `http://127.0.0.1:8790/cards`.
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
}

impl HttpRequest {
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: BTreeMap::new(),
            body: None,
        }
    }

    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }
}

/// A single HTTP response.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Anything that can turn an [`HttpRequest`] into an [`HttpResponse`].
pub trait Transport: Send + Sync {
    fn request(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// Default transport: plain HTTP over `std::net::TcpStream`.
///
/// Deliberately minimal — one request per connection (`Connection: close`), no
/// redirect following, no chunked request bodies. That is sufficient for the
/// JSON endpoints this SDK talks to, and keeps the code auditable.
#[derive(Debug, Clone)]
pub struct StdHttpTransport {
    pub timeout: Duration,
}

impl Default for StdHttpTransport {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }
}

impl StdHttpTransport {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Transport for StdHttpTransport {
    fn request(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let parsed = ParsedUrl::parse(&request.url)?;
        if parsed.scheme != "http" {
            return Err(HttpError::new(format!(
                "unsupported URL scheme '{}': this crate ships no TLS. \
                 Use http:// or implement the Transport trait over your own client",
                parsed.scheme
            )));
        }

        let address = format!("{}:{}", parsed.host, parsed.port);
        let mut stream = TcpStream::connect(&address).map_err(|e| {
            HttpError::new(format!("connect to {} failed: {}", address, e))
        })?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|e| HttpError::new(format!("set_read_timeout failed: {}", e)))?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|e| HttpError::new(format!("set_write_timeout failed: {}", e)))?;

        let mut head = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: */*\r\n",
            request.method, parsed.path_query, parsed.host_header()
        );
        for (key, value) in &request.headers {
            head.push_str(&format!("{}: {}\r\n", key, value));
        }
        let body_bytes = request.body.as_deref().unwrap_or("").as_bytes().to_vec();
        if request.body.is_some() {
            head.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
        }
        head.push_str("\r\n");

        stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.write_all(&body_bytes))
            .and_then(|_| stream.flush())
            .map_err(|e| HttpError::new(format!("write to {} failed: {}", address, e)))?;

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|e| HttpError::new(format!("read from {} failed: {}", address, e)))?;

        parse_response(&raw)
    }
}

/// Split an HTTP/1.1 response into status + body.
///
/// Handles `Content-Length` and `Transfer-Encoding: chunked`; if neither is
/// present it takes the remainder (we always send `Connection: close`).
fn parse_response(raw: &[u8]) -> Result<HttpResponse, HttpError> {
    let header_end = find_subsequence(raw, b"\r\n\r\n")
        .ok_or_else(|| HttpError::new("response has no header terminator"))?;
    let head = std::str::from_utf8(&raw[..header_end])
        .map_err(|e| HttpError::new(format!("response header is not UTF-8: {}", e)))?;
    let mut lines = head.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| HttpError::new("response has no status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| HttpError::new(format!("malformed status line: {:?}", status_line)))?;

    let mut chunked = false;
    let mut content_length: Option<usize> = None;
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if key == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        } else if key == "content-length" {
            content_length = value.parse::<usize>().ok();
        }
    }

    let body_start = header_end + 4;
    let body_raw = &raw[body_start..];
    let body = if chunked {
        decode_chunked(body_raw)?
    } else if let Some(length) = content_length {
        let end = (body_start + length).min(raw.len());
        raw[body_start..end].to_vec()
    } else {
        body_raw.to_vec()
    };

    let text = String::from_utf8_lossy(&body).to_string();
    Ok(HttpResponse::new(status, text))
}

fn decode_chunked(raw: &[u8]) -> Result<Vec<u8>, HttpError> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let line_end = find_subsequence(&raw[pos..], b"\r\n")
            .ok_or_else(|| HttpError::new("chunked body missing size line"))?;
        let size_line = std::str::from_utf8(&raw[pos..pos + line_end])
            .map_err(|e| HttpError::new(format!("chunk size is not UTF-8: {}", e)))?;
        // Ignore chunk extensions after ';'.
        let size_text = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| HttpError::new(format!("malformed chunk size: {:?}", size_text)))?;
        pos += line_end + 2;
        if size == 0 {
            break;
        }
        if pos + size > raw.len() {
            return Err(HttpError::new("chunked body truncated"));
        }
        out.extend_from_slice(&raw[pos..pos + size]);
        pos += size + 2; // skip trailing CRLF
    }
    Ok(out)
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path_query: String,
}

impl ParsedUrl {
    fn parse(url: &str) -> Result<Self, HttpError> {
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| HttpError::new(format!("URL is missing a scheme: {}", url)))?;
        let scheme = scheme.to_ascii_lowercase();
        let default_port = match scheme.as_str() {
            "http" => 80,
            "https" => 443,
            other => {
                return Err(HttpError::new(format!(
                    "unsupported URL scheme '{}'",
                    other
                )))
            }
        };
        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port_text)) if !port_text.is_empty() => {
                let port = port_text.parse::<u16>().map_err(|_| {
                    HttpError::new(format!("invalid port in URL: {}", authority))
                })?;
                (host.to_string(), port)
            }
            _ => (authority.to_string(), default_port),
        };
        if host.is_empty() {
            return Err(HttpError::new(format!("URL has no host: {}", url)));
        }
        Ok(Self {
            scheme,
            host,
            port,
            path_query: path.to_string(),
        })
    }

    fn host_header(&self) -> String {
        let default = match self.scheme.as_str() {
            "https" => 443,
            _ => 80,
        };
        if self.port == default {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// In-memory transport for tests and for callers who already have a client.
///
/// Queue up responses; every request is recorded so tests can assert on the
/// exact URL, headers and body that were produced.
#[derive(Debug, Default)]
pub struct MockTransport {
    responses: std::sync::Mutex<Vec<Result<HttpResponse, String>>>,
    pub requests: std::sync::Mutex<Vec<HttpRequest>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a response. Responses are consumed in order.
    pub fn push(&self, status: u16, body: impl Into<String>) -> &Self {
        self.responses
            .lock()
            .expect("mock transport mutex poisoned")
            .push(Ok(HttpResponse::new(status, body)));
        self
    }

    /// Queue a transport failure (as opposed to an HTTP error status).
    pub fn push_error(&self, message: impl Into<String>) -> &Self {
        self.responses
            .lock()
            .expect("mock transport mutex poisoned")
            .push(Err(message.into()));
        self
    }

    /// Requests seen so far.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests
            .lock()
            .expect("mock transport mutex poisoned")
            .clone()
    }

    pub fn last_request(&self) -> Option<HttpRequest> {
        self.requests
            .lock()
            .expect("mock transport mutex poisoned")
            .last()
            .cloned()
    }
}

impl Transport for MockTransport {
    fn request(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.requests
            .lock()
            .expect("mock transport mutex poisoned")
            .push(request.clone());
        let mut responses = self
            .responses
            .lock()
            .expect("mock transport mutex poisoned");
        if responses.is_empty() {
            return Err(HttpError::new(
                "MockTransport ran out of queued responses".to_string(),
            ));
        }
        match responses.remove(0) {
            Ok(response) => Ok(response),
            Err(message) => Err(HttpError::new(message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_url_with_default_and_explicit_ports() {
        let a = ParsedUrl::parse("http://127.0.0.1/cards").unwrap();
        assert_eq!(a.host, "127.0.0.1");
        assert_eq!(a.port, 80);
        assert_eq!(a.path_query, "/cards");
        assert_eq!(a.host_header(), "127.0.0.1");

        let b = ParsedUrl::parse("http://127.0.0.1:8790/search?x=1").unwrap();
        assert_eq!(b.port, 8790);
        assert_eq!(b.path_query, "/search?x=1");
        assert_eq!(b.host_header(), "127.0.0.1:8790");
    }

    #[test]
    fn https_is_rejected_explicitly() {
        let error = ParsedUrl::parse("https://example.org/x").unwrap();
        assert_eq!(error.scheme, "https");
        // The transport (not the parser) refuses it, with a helpful message.
        let transport = StdHttpTransport::default();
        let request = HttpRequest::new("GET", "https://example.org/x");
        let message = transport.request(&request).unwrap_err().to_string();
        assert!(message.contains("no TLS"), "got: {}", message);
    }

    #[test]
    fn parses_content_length_response() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhelloEXTRA";
        let response = parse_response(raw).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, "hello");
    }

    #[test]
    fn parses_chunked_response() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let response = parse_response(raw).unwrap();
        assert_eq!(response.body, "hello");
    }

    #[test]
    fn parses_response_without_length_as_remainder() {
        let raw = b"HTTP/1.1 204 No Content\r\n\r\n";
        let response = parse_response(raw).unwrap();
        assert_eq!(response.status, 204);
        assert_eq!(response.body, "");
    }

    #[test]
    fn mock_transport_records_requests() {
        let transport = MockTransport::new();
        transport.push(200, "{}");
        let request = HttpRequest::new("POST", "http://x/y").body("{}");
        transport.request(&request).unwrap();
        assert_eq!(transport.requests().len(), 1);
        assert_eq!(transport.last_request().unwrap().method, "POST");
    }
}
