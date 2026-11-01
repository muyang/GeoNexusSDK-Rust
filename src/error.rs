//! Error types, mirroring the Python SDK's exception hierarchy.
//!
//! The distinction that matters most here is **protocol level vs application
//! level**: the caller must be able to tell "the transport/JSON-RPC envelope
//! broke" from "the server understood me and refused". Retry logic differs.

use std::fmt;

/// A GeoCard failed contract validation. Carries every error, not just the first.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoCardValidationError {
    pub errors: Vec<String>,
}

impl GeoCardValidationError {
    pub fn new(errors: Vec<String>) -> Self {
        Self { errors }
    }
}

impl fmt::Display for GeoCardValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GeoCard validation failed:")?;
        for error in &self.errors {
            write!(f, "\n- {}", error)?;
        }
        Ok(())
    }
}

impl std::error::Error for GeoCardValidationError {}

/// JSON-RPC 2.0 reserved codes. These are **negative** and must not be invented.
pub mod jsonrpc {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
}

/// GeoNexus application codes (positive 2xxx space).
pub mod app {
    pub const CONTRACT_NOT_SATISFIED: i64 = 2000;
    pub const SKILL_NOT_FOUND: i64 = 2001;
    pub const GEOCARD_NOT_FOUND: i64 = 2002;
    pub const EXECUTION_FAILED: i64 = 2003;
    pub const INVALID_ARGUMENT: i64 = 2004;
    pub const RECIPE_NOT_FOUND: i64 = 2005;
    pub const RECIPE_PARAM_INVALID: i64 = 2006;
    pub const CONTRACT_BINDING_REJECTED: i64 = 2007;
    pub const PUSHDOWN_UNRESOLVED: i64 = 2008;
    pub const FEDERATION_UNRESOLVED: i64 = 2009;
}

/// An error returned by a GeoMCP server, or a transport failure talking to one.
#[derive(Debug, Clone)]
pub struct GeoMcpError {
    pub code: i64,
    pub message: String,
    pub data: Option<super::json::Json>,
}

impl GeoMcpError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(code: i64, message: impl Into<String>, data: super::json::Json) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    /// True for the JSON-RPC reserved range (transport/protocol level).
    pub fn is_protocol_level(&self) -> bool {
        self.code < 0
    }

    /// Human-readable category, useful in logs.
    pub fn category(&self) -> &'static str {
        match self.code {
            app::CONTRACT_NOT_SATISFIED => "contract-not-satisfied",
            app::SKILL_NOT_FOUND => "skill-not-found",
            app::GEOCARD_NOT_FOUND => "geocard-not-found",
            app::EXECUTION_FAILED => "execution-failed",
            app::INVALID_ARGUMENT => "invalid-argument",
            app::RECIPE_NOT_FOUND => "recipe-not-found",
            app::RECIPE_PARAM_INVALID => "recipe-param-invalid",
            app::CONTRACT_BINDING_REJECTED => "contract-binding-rejected",
            app::PUSHDOWN_UNRESOLVED => "pushdown-unresolved",
            app::FEDERATION_UNRESOLVED => "federation-unresolved",
            jsonrpc::PARSE_ERROR => "parse-error",
            jsonrpc::INVALID_REQUEST => "invalid-request",
            jsonrpc::METHOD_NOT_FOUND => "method-not-found",
            jsonrpc::INVALID_PARAMS => "invalid-params",
            jsonrpc::INTERNAL_ERROR => "internal-error",
            _ => "unknown-error",
        }
    }
}

impl fmt::Display for GeoMcpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for GeoMcpError {}

/// A GeoCard Registry rejected or failed a request.
#[derive(Debug, Clone)]
pub struct RegistryError {
    pub message: String,
    pub status: Option<u16>,
    pub body: Option<String>,
}

impl RegistryError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            body: None,
        }
    }

    pub fn with_status(message: impl Into<String>, status: u16, body: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: Some(status),
            body: Some(body.into()),
        }
    }
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.status {
            Some(status) => write!(f, "Registry error {}: {}", status, self.message),
            None => write!(f, "Registry error: {}", self.message),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Base OGE failure.
#[derive(Debug, Clone)]
pub struct OgeError {
    pub message: String,
    pub status: Option<u16>,
    pub body: Option<String>,
}

impl OgeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            body: None,
        }
    }

    pub fn with_status(message: impl Into<String>, status: u16, body: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: Some(status),
            body: Some(body.into()),
        }
    }
}

impl fmt::Display for OgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OgeError {}

/// OGE authentication failure (token missing, expired, or rejected).
#[derive(Debug, Clone)]
pub struct OgeAuthError {
    pub message: String,
    pub status: Option<u16>,
}

impl OgeAuthError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
        }
    }
}

impl fmt::Display for OgeAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OgeAuthError {}

/// An OGE execution failed, and we know (or do not know) the process id.
#[derive(Debug, Clone)]
pub struct OgeExecutionError {
    pub message: String,
    pub process_id: Option<String>,
    pub status: Option<u16>,
}

impl OgeExecutionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            process_id: None,
            status: None,
        }
    }
}

impl fmt::Display for OgeExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OgeExecutionError {}

/// Everything that can go wrong in one client call.
///
/// A single enum keeps `?` usable everywhere without a `Box<dyn Error>`; the
/// variants mirror the Python exception classes so logs read the same.
#[derive(Debug)]
pub enum Error {
    Json(super::json::JsonError),
    Http(HttpError),
    GeoCard(GeoCardValidationError),
    GeoMcp(GeoMcpError),
    Registry(RegistryError),
    Oge(OgeError),
    OgeAuth(OgeAuthError),
    OgeExecution(OgeExecutionError),
    /// Local input rejection (bad argument, disallowed character, ...).
    InvalidArgument(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Json(e) => write!(f, "JSON error: {}", e),
            Error::Http(e) => write!(f, "HTTP error: {}", e),
            Error::GeoCard(e) => write!(f, "{}", e),
            Error::GeoMcp(e) => write!(f, "{}", e),
            Error::Registry(e) => write!(f, "{}", e),
            Error::Oge(e) => write!(f, "{}", e),
            Error::OgeAuth(e) => write!(f, "{}", e),
            Error::OgeExecution(e) => write!(f, "{}", e),
            Error::InvalidArgument(message) => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for Error {}

impl From<super::json::JsonError> for Error {
    fn from(value: super::json::JsonError) -> Self {
        Error::Json(value)
    }
}

impl From<HttpError> for Error {
    fn from(value: HttpError) -> Self {
        Error::Http(value)
    }
}

impl From<GeoCardValidationError> for Error {
    fn from(value: GeoCardValidationError) -> Self {
        Error::GeoCard(value)
    }
}

impl From<GeoMcpError> for Error {
    fn from(value: GeoMcpError) -> Self {
        Error::GeoMcp(value)
    }
}

impl From<RegistryError> for Error {
    fn from(value: RegistryError) -> Self {
        Error::Registry(value)
    }
}

impl From<OgeError> for Error {
    fn from(value: OgeError) -> Self {
        Error::Oge(value)
    }
}

impl From<OgeAuthError> for Error {
    fn from(value: OgeAuthError) -> Self {
        Error::OgeAuth(value)
    }
}

impl From<OgeExecutionError> for Error {
    fn from(value: OgeExecutionError) -> Self {
        Error::OgeExecution(value)
    }
}

/// Transport-level failure from the built-in HTTP client.
#[derive(Debug, Clone)]
pub struct HttpError {
    pub message: String,
}

impl HttpError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for HttpError {}
