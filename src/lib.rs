//! # tonic-debug
//!
//! A debugging and diagnostics middleware for [tonic](https://github.com/hyperium/tonic)
//! gRPC servers.
//!
//! ## Overview
//!
//! Debugging gRPC services built with tonic can be challenging — messages are
//! binary-encoded protobufs, connection issues are opaque, and the standard
//! logging often lacks the detail needed to quickly diagnose problems.
//!
//! `tonic-debug` solves this by providing:
//!
//! - **Human-readable protobuf inspection** — decodes protobuf wire format
//!   without needing `.proto` schemas, showing field numbers, types, and values.
//! - **Connection lifecycle observability** — tracks connection events (connect,
//!   disconnect, errors) with active/total counters via tower/hyper integration.
//! - **Structured logging via `tracing`** — all output goes through the
//!   `tracing` crate with structured fields for easy filtering and aggregation.
//! - **Optional OpenTelemetry export** — enable the `opentelemetry` feature to
//!   forward spans and attributes to your OTel collector.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use tonic_debug::DebugLayer;
//!
//! // Add the debug layer to your tonic server
//! // tonic::transport::Server::builder()
//! //     .layer(DebugLayer::new())
//! //     .add_service(my_service)
//! //     .serve(addr)
//! //     .await?;
//! ```
//!
//! ## Configuration
//!
//! ```rust
//! use tonic_debug::{DebugLayer, DebugConfig};
//!
//! let layer = DebugLayer::with_config(DebugConfig {
//!     log_headers: true,
//!     log_bodies: true,
//!     log_response_frames: true,
//!     max_body_bytes: 8192,
//!     hex_dump: false,
//! });
//! ```
//!
//! ## Connection Tracking
//!
//! ```rust
//! use tonic_debug::ConnectionMetrics;
//!
//! let metrics = ConnectionMetrics::new();
//! // Use metrics.active_connections(), metrics.total_connections(), etc.
//! ```
//!
//! ## Feature Flags
//!
//! | Feature        | Description                                      |
//! |----------------|--------------------------------------------------|
//! | `opentelemetry`| Adds OpenTelemetry span attributes and export    |

pub mod body;
pub mod connection;
pub mod inspect;
pub mod layer;
#[cfg(feature = "opentelemetry")]
pub mod otel;
pub mod service;

// Re-export primary types for ergonomic usage.
pub use connection::{ConnectionGuard, ConnectionMetrics, ConnectionTrackerLayer};
pub use layer::{DebugConfig, DebugLayer};
pub use service::DebugService;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_layer_default() {
        let layer = DebugLayer::new();
        // Should compile and create without panicking.
        let _ = layer;
    }

    #[test]
    fn test_debug_layer_builder() {
        let layer = DebugLayer::new()
            .log_headers(false)
            .log_bodies(true)
            .log_response_frames(false)
            .max_body_bytes(1024)
            .hex_dump(true);
        let _ = layer;
    }

    #[test]
    fn test_debug_config_default() {
        let config = DebugConfig::default();
        assert!(config.log_headers);
        assert!(config.log_bodies);
        assert!(config.log_response_frames);
        assert_eq!(config.max_body_bytes, 4096);
        assert!(!config.hex_dump);
    }

    #[test]
    fn test_connection_metrics_default() {
        let metrics = ConnectionMetrics::new();
        assert_eq!(metrics.active_connections(), 0);
        assert_eq!(metrics.total_connections(), 0);
        assert_eq!(metrics.connection_errors(), 0);
    }
}
