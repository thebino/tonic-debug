//! Tower Layer implementation for the gRPC debug middleware.
//!
//! [`DebugLayer`] is the primary entry point for users of this crate. It wraps
//! any tonic/tower service with request/response inspection and logging.

use tower_layer::Layer;

use crate::service::DebugService;

/// Configuration for the gRPC debug middleware.
#[derive(Debug, Clone)]
pub struct DebugLayer {
    config: DebugConfig,
}

/// Controls what the debug middleware captures and logs.
#[derive(Debug, Clone)]
pub struct DebugConfig {
    /// Log request headers.
    pub log_headers: bool,
    /// Log request and response bodies (protobuf inspection).
    pub log_bodies: bool,
    /// Log individual response body frames as they stream.
    pub log_response_frames: bool,
    /// Maximum number of bytes to capture for body inspection.
    pub max_body_bytes: usize,
    /// Include a hex dump of raw bytes in logs.
    pub hex_dump: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            log_headers: true,
            log_bodies: true,
            log_response_frames: true,
            max_body_bytes: 4096,
            hex_dump: false,
        }
    }
}

impl DebugLayer {
    /// Create a new `DebugLayer` with default configuration.
    ///
    /// By default, headers and bodies are logged, hex dumps are off.
    pub fn new() -> Self {
        Self {
            config: DebugConfig::default(),
        }
    }

    /// Create a new `DebugLayer` with the given configuration.
    pub fn with_config(config: DebugConfig) -> Self {
        Self { config }
    }

    /// Set whether to log request/response headers.
    pub fn log_headers(mut self, enabled: bool) -> Self {
        self.config.log_headers = enabled;
        self
    }

    /// Set whether to log request/response bodies.
    pub fn log_bodies(mut self, enabled: bool) -> Self {
        self.config.log_bodies = enabled;
        self
    }

    /// Set whether to log individual response body frames.
    pub fn log_response_frames(mut self, enabled: bool) -> Self {
        self.config.log_response_frames = enabled;
        self
    }

    /// Set the maximum number of bytes to capture for body inspection.
    pub fn max_body_bytes(mut self, max: usize) -> Self {
        self.config.max_body_bytes = max;
        self
    }

    /// Set whether to include hex dumps in log output.
    pub fn hex_dump(mut self, enabled: bool) -> Self {
        self.config.hex_dump = enabled;
        self
    }
}

impl Default for DebugLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for DebugLayer {
    type Service = DebugService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DebugService::new(inner, self.config.clone())
    }
}
