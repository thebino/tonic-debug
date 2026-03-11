//! Response body wrapper for capturing gRPC response data.
//!
//! Wraps the inner HTTP response body so that frames can be inspected
//! and logged as they stream through the middleware.

use bytes::Bytes;
use http_body::Frame;
use pin_project_lite::pin_project;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tracing;

use crate::inspect;

pin_project! {
    /// A wrapper around an HTTP response body that logs gRPC frames as they are streamed.
    pub struct DebugBody<B> {
        #[pin]
        inner: B,
        method: String,
        log_body: bool,
        max_capture_bytes: usize,
        captured: Vec<u8>,
    }
}

impl<B> DebugBody<B> {
    /// Create a new `DebugBody` wrapping the given body.
    pub fn new(inner: B, method: String, log_body: bool, max_capture_bytes: usize) -> Self {
        Self {
            inner,
            method,
            log_body,
            max_capture_bytes,
            captured: Vec::new(),
        }
    }
}

impl<B> http_body::Body for DebugBody<B>
where
    B: http_body::Body<Data = Bytes>,
    B::Error: std::fmt::Display,
{
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.project();

        match this.inner.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if *this.log_body {
                    if let Some(data) = frame.data_ref() {
                        let bytes = data.as_ref();
                        // Accumulate bytes for inspection
                        let remaining = this.max_capture_bytes.saturating_sub(this.captured.len());
                        let to_capture = bytes.len().min(remaining);
                        this.captured.extend_from_slice(&bytes[..to_capture]);

                        let formatted = inspect::format_grpc_message(bytes);
                        tracing::debug!(
                            method = %this.method,
                            frame_size = bytes.len(),
                            "gRPC response frame:\n{}",
                            formatted
                        );
                    }

                    if let Some(trailers) = frame.trailers_ref() {
                        let grpc_status = trailers
                            .get("grpc-status")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("unknown");
                        let grpc_message = trailers
                            .get("grpc-message")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("");

                        if grpc_status != "0" {
                            tracing::warn!(
                                method = %this.method,
                                grpc_status = grpc_status,
                                grpc_message = grpc_message,
                                "gRPC response trailers indicate error"
                            );
                        } else {
                            tracing::debug!(
                                method = %this.method,
                                grpc_status = grpc_status,
                                "gRPC response trailers OK"
                            );
                        }
                    }
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(e))) => {
                tracing::error!(
                    method = %this.method,
                    error = %e,
                    "gRPC response body error"
                );
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                if *this.log_body && !this.captured.is_empty() {
                    tracing::trace!(
                        method = %this.method,
                        total_response_bytes = this.captured.len(),
                        "gRPC response stream completed"
                    );
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}
