//! # GeoNexus SDK for Rust
//!
//! Parity-targeted port of **GeoNexus-SDK v1.1.1** (Python), covering the
//! *core* layers — the ones that "know only cards and the protocol":
//!
//! | Module | Role | Python equivalent |
//! |---|---|---|
//! | [`geocard`] | contract: types, validation, builder | `geonexus.geocard` |
//! | [`geomcp`] | protocol: JSON-RPC envelope, methods, error codes | `geonexus.geomcp.client` |
//! | [`registry`] | discovery: register, search, review | `geonexus.registry.client` |
//! | [`transport`] | pluggable HTTP (std client + mock) | `httpx` usage |
//! | [`json`] | JSON value, parser, serializer | `json` |
//! | [`error`] | error types and code spaces | exception classes |
//!
//! ## What is deliberately absent
//!
//! The OGE adapter, the agent planner, task-graph state machines, recipe
//! engines, GAAG and federation pushdown are **not** ported here. Adapters sit
//! at the edge by design (see `docs/BOUNDARIES.md`), and porting them without
//! being able to compile and test them would be an unverified claim rather than
//! a feature. The TypeScript port carries the OGE adapter; this crate covers
//! core only.
//!
//! ## No dependencies, and what that costs
//!
//! This crate uses **std only** (see `Cargo.toml`). The benefit is a tiny
//! supply chain and trivial embedding. The cost is that the default
//! [`transport::StdHttpTransport`] speaks **plain HTTP only** — `std` has no
//! TLS. GeoMCP nodes, registries and OGE all run on `http://` in this project,
//! so that is sufficient here. If you need HTTPS, implement
//! [`transport::Transport`] over your own client: none of the protocol or
//! card logic depends on the transport.
//!
//! ## Example
//!
//! ```no_run
//! use geonexus::{json::Json, transport::StdHttpTransport, GeoMcpClient, RegistryClient};
//!
//! let transport = StdHttpTransport::default();
//!
//! // Ask a node what it can do.
//! let node = GeoMcpClient::new("http://127.0.0.1:8787", &transport);
//! let capabilities = node.capabilities().unwrap();
//! println!("{}", capabilities.to_string_pretty());
//!
//! // Execute a skill.
//! let params = Json::object([
//!     ("skill", Json::string("oge-ndvi")),
//!     (
//!         "params",
//!         Json::object([("coverage", Json::string("Platform:Coverage:img:prod"))]),
//!     ),
//! ]);
//! let result = node.execute(params).unwrap();
//!
//! // Make a card discoverable.
//! let registry = RegistryClient::new("http://127.0.0.1:8790", &transport);
//! let hits = registry.search(&[("capability", "ndvi")]).unwrap();
//! println!("{} card(s) match", hits.len());
//! ```
//!
//! ## Card validation: strict and permissive are separate
//!
//! ```no_run
//! use geonexus::{geocard::{parse_card, validate_card, GeoCardBuilder}, json::Json};
//!
//! let card = GeoCardBuilder::new("asset-1", "data", "Asset", "desc")
//!     .spatial(vec![100.0, 20.0, 110.0, 30.0], "EPSG:4326")
//!     .capability("ndvi")
//!     .build()
//!     .expect("valid card");
//!
//! // Inspector mode: do not enforce the contract, so malformed cards from
//! // elsewhere can still be read and repaired.
//! let loose = parse_card(card.clone()).unwrap();
//! assert_eq!(loose, card);
//! ```
//!
//! The published schema (`schemas/geocard.schema.json`) is embedded at compile
//! time and a test asserts the type enum here matches it, so this port cannot
//! drift from Python/TypeScript without failing.

pub mod error;
pub mod geocard;
pub mod geomcp;
pub mod json;
pub mod registry;
pub mod transport;

pub use error::{
    app, jsonrpc, Error, GeoCardValidationError, GeoMcpError, HttpError, OgeAuthError, OgeError,
    OgeExecutionError, RegistryError,
};
pub use geocard::{capability_names, card_errors, parse_card, validate_card, GeoCardBuilder, GEOCARD_TYPES};
pub use geomcp::{advertised_skills, build_request, parse_envelope, GeoMcpClient, METHODS, WRITE_METHODS};
pub use json::Json;
pub use registry::RegistryClient;
pub use transport::{HttpRequest, HttpResponse, MockTransport, StdHttpTransport, Transport};

/// SDK version, matching the Python package version.
pub const VERSION: &str = "1.1.1";
