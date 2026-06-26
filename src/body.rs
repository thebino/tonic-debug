//! Request/response body wrapper for capturing gRPC body data.
//!
//! Wraps an inner HTTP body so that frames can be inspected and logged as
//! they stream through the middleware, in either direction.

use bytes::Bytes;
use http_body::Frame;
use pin_project_lite::pin_project;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tracing;

use crate::inspect;

/// Which side of the call a [`DebugBody`] is wrapping, used to label log output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// An inbound request body being read by the inner service.
    Request,
    /// An outbound response body being streamed back to the caller.
    Response,
}

impl Direction {
    /// Lowercase label used in log messages ("request" / "response").
    fn as_str(self) -> &'static str {
        match self {
            Direction::Request => "request",
            Direction::Response => "response",
        }
    }
}

pin_project! {
    /// A wrapper around an HTTP body that logs gRPC frames as they are streamed.
    pub struct DebugBody<B> {
        #[pin]
        inner: B,
        method: String,
        // Which direction this body flows, for log labelling.
        direction: Direction,
        // Master switch: inspect body contents at all (maps to `DebugConfig::log_bodies`).
        log_body: bool,
        // Log each streamed frame as it passes.
        log_frames: bool,
        // Render captured bytes as a hex dump instead of decoded protobuf.
        hex_dump: bool,
        max_capture_bytes: usize,
        captured: Vec<u8>,
    }
}

impl<B> DebugBody<B> {
    /// Create a new `DebugBody` wrapping the given body.
    pub fn new(
        inner: B,
        method: String,
        direction: Direction,
        log_body: bool,
        log_frames: bool,
        hex_dump: bool,
        max_capture_bytes: usize,
    ) -> Self {
        Self {
            inner,
            method,
            direction,
            log_body,
            log_frames,
            hex_dump,
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
        let dir = this.direction.as_str();

        match this.inner.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if *this.log_body {
                    if let Some(data) = frame.data_ref() {
                        let bytes = data.as_ref();
                        // Accumulate bytes for inspection
                        let remaining = this.max_capture_bytes.saturating_sub(this.captured.len());
                        let to_capture = bytes.len().min(remaining);
                        this.captured.extend_from_slice(&bytes[..to_capture]);

                        if *this.log_frames {
                            let formatted = if *this.hex_dump {
                                inspect::hex_dump(bytes, *this.max_capture_bytes)
                            } else {
                                inspect::format_grpc_message(bytes)
                            };
                            tracing::debug!(
                                method = %this.method,
                                direction = dir,
                                frame_size = bytes.len(),
                                "gRPC {} frame:\n{}",
                                dir,
                                formatted
                            );
                        }
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
                                direction = dir,
                                grpc_status = grpc_status,
                                grpc_message = grpc_message,
                                "gRPC {} trailers indicate error",
                                dir
                            );
                        } else {
                            tracing::debug!(
                                method = %this.method,
                                direction = dir,
                                grpc_status = grpc_status,
                                "gRPC {} trailers OK",
                                dir
                            );
                        }
                    }
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(e))) => {
                tracing::error!(
                    method = %this.method,
                    direction = dir,
                    error = %e,
                    "gRPC {} body error",
                    dir
                );
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                if *this.log_body && !this.captured.is_empty() {
                    tracing::trace!(
                        method = %this.method,
                        direction = dir,
                        total_bytes = this.captured.len(),
                        "gRPC {} stream completed",
                        dir
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
