//! Tower Service implementation for gRPC request/response interception.
//!
//! [`DebugService`] intercepts every gRPC call, logs request metadata and
//! body contents, forwards to the inner service, and then logs the response.

use bytes::Bytes;
use http::{Request, Response};
use http_body::Body as HttpBody;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower_service::Service;

use crate::body::DebugBody;
use crate::layer::DebugConfig;

/// A Tower service that intercepts and logs gRPC requests and responses.
#[derive(Debug, Clone)]
pub struct DebugService<S> {
    inner: S,
    config: DebugConfig,
}

impl<S> DebugService<S> {
    /// Create a new `DebugService` wrapping the given service.
    pub fn new(inner: S, config: DebugConfig) -> Self {
        Self { inner, config }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for DebugService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::fmt::Display + Send + 'static,
    ReqBody: HttpBody<Data = Bytes> + Send + 'static,
    ReqBody::Error: std::fmt::Display,
    ResBody: HttpBody<Data = Bytes> + Send + 'static,
    ResBody::Error: std::fmt::Display + Send,
{
    type Response = Response<DebugBody<ResBody>>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let config = self.config.clone();
        let mut inner = self.inner.clone();
        // Per tower best practice: swap the clone so the ready state is preserved.
        std::mem::swap(&mut self.inner, &mut inner);

        Box::pin(async move {
            let start = tokio::time::Instant::now();

            // Extract gRPC method path from the URI
            let method = req.uri().path().to_string();
            let http_method = req.method().clone();

            // Log request metadata
            tracing::info!(
                method = %method,
                http_method = %http_method,
                "→ gRPC request"
            );

            if config.log_headers {
                let headers = req.headers();
                let content_type = headers
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown");
                let authority = req
                    .uri()
                    .authority()
                    .map(|a| a.to_string())
                    .unwrap_or_default();
                let user_agent = headers
                    .get("user-agent")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown");
                let grpc_timeout = headers.get("grpc-timeout").and_then(|v| v.to_str().ok());

                // Log custom metadata (headers not starting with standard prefixes)
                let custom_metadata: Vec<_> = headers
                    .iter()
                    .filter(|(name, _)| {
                        let n = name.as_str();
                        !n.starts_with(':')
                            && n != "content-type"
                            && n != "user-agent"
                            && n != "te"
                            && n != "grpc-timeout"
                            && n != "grpc-encoding"
                            && n != "grpc-accept-encoding"
                    })
                    .map(|(name, value)| {
                        if !config.reveal_sensitive_headers
                            && config.sensitive_headers.contains(name)
                        {
                            format!("{}=[REDACTED]", name.as_str())
                        } else {
                            format!("{}={}", name.as_str(), value.to_str().unwrap_or("<binary>"))
                        }
                    })
                    .collect();

                if !custom_metadata.is_empty() {
                    tracing::debug!(
                        method = %method,
                        metadata = ?custom_metadata,
                        content_type = content_type,
                        authority = %authority,
                        user_agent = user_agent,
                        grpc_timeout = ?grpc_timeout,
                        "→ gRPC request headers"
                    );
                } else {
                    tracing::debug!(
                        method = %method,
                        content_type = content_type,
                        authority = %authority,
                        user_agent = user_agent,
                        grpc_timeout = ?grpc_timeout,
                        "→ gRPC request headers"
                    );

                }
            }

            // Call the inner service
            let response = inner.call(req).await;

            let elapsed = start.elapsed();

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    let grpc_status = resp
                        .headers()
                        .get("grpc-status")
                        .and_then(|v| v.to_str().ok())
                        .map(String::from);

                    if let Some(ref gs) = grpc_status {
                        if gs != "0" {
                            let grpc_message = resp
                                .headers()
                                .get("grpc-message")
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("");
                            tracing::warn!(
                                method = %method,
                                http_status = %status,
                                grpc_status = %gs,
                                grpc_message = grpc_message,
                                elapsed_ms = elapsed.as_millis() as u64,
                                "← gRPC response (error)"
                            );
                        } else {
                            tracing::info!(
                                method = %method,
                                http_status = %status,
                                grpc_status = %gs,
                                elapsed_ms = elapsed.as_millis() as u64,
                                "← gRPC response"
                            );
                        }
                    } else {
                        // gRPC status will come in trailers
                        tracing::info!(
                            method = %method,
                            http_status = %status,
                            elapsed_ms = elapsed.as_millis() as u64,
                            "← gRPC response (status in trailers)"
                        );
                    }

                    if config.log_headers {
                        tracing::debug!(
                            method = %method,
                            headers = ?resp.headers(),
                            "← gRPC response headers"
                        );
                    }

                    // Wrap the response body for frame-level logging
                    let (parts, body) = resp.into_parts();
                    let debug_body = DebugBody::new(
                        body,
                        method,
                        config.log_response_frames,
                        config.max_body_bytes,
                    );
                    Ok(Response::from_parts(parts, debug_body))
                }
                Err(e) => {
                    tracing::error!(
                        method = %method,
                        elapsed_ms = elapsed.as_millis() as u64,
                        error = %e,
                        "← gRPC call failed"
                    );
                    Err(e)
                }
            }
        })
    }
}
