//! OpenTelemetry integration for gRPC debugging.
//!
//! When the `opentelemetry` feature is enabled, this module provides utilities
//! for enriching traces and spans with gRPC debugging information.
//!
//! This module works alongside the standard `tracing` integration — the
//! [`DebugLayer`](crate::DebugLayer) always emits `tracing` events, and when
//! OpenTelemetry is configured, those events are automatically exported to
//! your OTel collector via `tracing-opentelemetry`.

use tracing;

/// Semantic convention attribute keys for gRPC spans.
///
/// These follow the OpenTelemetry semantic conventions for gRPC where
/// applicable, with additional keys specific to debugging.
pub mod attributes {
    /// The full gRPC method path (e.g., `/mypackage.MyService/MyMethod`).
    pub const GRPC_METHOD: &str = "rpc.method";
    /// The gRPC service name (e.g., `mypackage.MyService`).
    pub const GRPC_SERVICE: &str = "rpc.service";
    /// The RPC system — always `"grpc"`.
    pub const RPC_SYSTEM: &str = "rpc.system";
    /// The gRPC status code as an integer.
    pub const GRPC_STATUS_CODE: &str = "rpc.grpc.status_code";
    /// The size of the request message in bytes.
    pub const GRPC_REQUEST_SIZE: &str = "rpc.grpc.request.size";
    /// The size of the response message in bytes.
    pub const GRPC_RESPONSE_SIZE: &str = "rpc.grpc.response.size";
    /// Number of request messages in a streaming call.
    pub const GRPC_REQUEST_COUNT: &str = "rpc.grpc.request.count";
    /// Number of response messages in a streaming call.
    pub const GRPC_RESPONSE_COUNT: &str = "rpc.grpc.response.count";
}

/// Parse a gRPC method path into (service, method) components.
///
/// For example, `/mypackage.MyService/MyMethod` returns
/// `Some(("mypackage.MyService", "MyMethod"))`.
pub fn parse_grpc_method(path: &str) -> Option<(&str, &str)> {
    let path = path.strip_prefix('/')?;
    let (service, method) = path.split_once('/')?;
    if service.is_empty() || method.is_empty() {
        return None;
    }
    Some((service, method))
}

/// Record gRPC-specific attributes on the current tracing span.
///
/// This function enriches the current span with OpenTelemetry semantic
/// convention attributes for gRPC calls. When `tracing-opentelemetry` is
/// configured, these attributes will be exported to your OTel backend.
pub fn record_grpc_attributes(method_path: &str) {
    if let Some((service, method)) = parse_grpc_method(method_path) {
        tracing::Span::current().record(attributes::RPC_SYSTEM, "grpc");
        tracing::Span::current().record(attributes::GRPC_SERVICE, service);
        tracing::Span::current().record(attributes::GRPC_METHOD, method);
    }
}

/// Record the gRPC status code on the current span.
pub fn record_grpc_status(status_code: i32) {
    tracing::Span::current().record(attributes::GRPC_STATUS_CODE, status_code);
}

/// Record request/response sizes on the current span.
pub fn record_message_sizes(request_bytes: Option<u64>, response_bytes: Option<u64>) {
    if let Some(req) = request_bytes {
        tracing::Span::current().record(attributes::GRPC_REQUEST_SIZE, req);
    }
    if let Some(resp) = response_bytes {
        tracing::Span::current().record(attributes::GRPC_RESPONSE_SIZE, resp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_grpc_method() {
        let result = parse_grpc_method("/mypackage.MyService/MyMethod");
        assert_eq!(result, Some(("mypackage.MyService", "MyMethod")));
    }

    #[test]
    fn test_parse_grpc_method_no_package() {
        let result = parse_grpc_method("/MyService/MyMethod");
        assert_eq!(result, Some(("MyService", "MyMethod")));
    }

    #[test]
    fn test_parse_grpc_method_invalid() {
        assert_eq!(parse_grpc_method(""), None);
        assert_eq!(parse_grpc_method("/"), None);
        assert_eq!(parse_grpc_method("no-leading-slash"), None);
        assert_eq!(parse_grpc_method("//EmptyService"), None);
        assert_eq!(parse_grpc_method("/Service/"), None);
    }
}
