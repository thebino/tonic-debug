//! Connection lifecycle observability.
//!
//! Provides a Tower layer that tracks TCP/HTTP2 connection events — new
//! connections, disconnections, and errors — giving operators visibility into
//! client connectivity issues.

use std::{
    fmt,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use tower_layer::Layer;
use tower_service::Service;

/// Shared connection metrics.
#[derive(Debug, Clone)]
pub struct ConnectionMetrics {
    inner: Arc<ConnectionMetricsInner>,
}

#[derive(Debug)]
struct ConnectionMetricsInner {
    /// Total number of connections ever accepted.
    total_connections: AtomicU64,
    /// Currently active connections.
    active_connections: AtomicU64,
    /// Total number of connection errors.
    connection_errors: AtomicU64,
}

impl ConnectionMetrics {
    /// Create a new `ConnectionMetrics` instance.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ConnectionMetricsInner {
                total_connections: AtomicU64::new(0),
                active_connections: AtomicU64::new(0),
                connection_errors: AtomicU64::new(0),
            }),
        }
    }

    /// Get the total number of connections ever accepted.
    pub fn total_connections(&self) -> u64 {
        self.inner.total_connections.load(Ordering::Relaxed)
    }

    /// Get the number of currently active connections.
    pub fn active_connections(&self) -> u64 {
        self.inner.active_connections.load(Ordering::Relaxed)
    }

    /// Get the total number of connection errors.
    pub fn connection_errors(&self) -> u64 {
        self.inner.connection_errors.load(Ordering::Relaxed)
    }

    fn on_connect(&self) {
        self.inner.total_connections.fetch_add(1, Ordering::Relaxed);
        self.inner
            .active_connections
            .fetch_add(1, Ordering::Relaxed);
    }

    fn on_disconnect(&self) {
        self.inner
            .active_connections
            .fetch_sub(1, Ordering::Relaxed);
    }

    fn on_error(&self) {
        self.inner.connection_errors.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for ConnectionMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConnectionMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "connections(total={}, active={}, errors={})",
            self.total_connections(),
            self.active_connections(),
            self.connection_errors()
        )
    }
}

/// A Tower layer that wraps a `MakeService` (or any per-connection service)
/// to track and log connection lifecycle events.
///
/// This should be applied at the server level, wrapping the service factory
/// that produces per-connection services.
#[derive(Debug, Clone)]
pub struct ConnectionTrackerLayer {
    metrics: ConnectionMetrics,
}

impl ConnectionTrackerLayer {
    /// Create a new `ConnectionTrackerLayer`.
    pub fn new() -> Self {
        Self {
            metrics: ConnectionMetrics::new(),
        }
    }

    /// Create a new `ConnectionTrackerLayer` with shared metrics.
    pub fn with_metrics(metrics: ConnectionMetrics) -> Self {
        Self { metrics }
    }

    /// Get a reference to the connection metrics.
    pub fn metrics(&self) -> &ConnectionMetrics {
        &self.metrics
    }
}

impl Default for ConnectionTrackerLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for ConnectionTrackerLayer {
    type Service = ConnectionTrackerService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ConnectionTrackerService {
            inner,
            metrics: self.metrics.clone(),
        }
    }
}

/// A Tower service that tracks connection lifecycle events.
///
/// When used as the outer service in a tonic server, each call to this
/// service represents a new connection being established.
#[derive(Debug, Clone)]
pub struct ConnectionTrackerService<S> {
    inner: S,
    metrics: ConnectionMetrics,
}

impl<S> ConnectionTrackerService<S> {
    /// Get a reference to the connection metrics.
    pub fn metrics(&self) -> &ConnectionMetrics {
        &self.metrics
    }
}

impl<S, Target> Service<Target> for ConnectionTrackerService<S>
where
    S: Service<Target> + Clone + Send + 'static,
    S::Response: Send + 'static,
    S::Error: fmt::Display + Send + 'static,
    S::Future: Send + 'static,
    Target: fmt::Debug + Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, target: Target) -> Self::Future {
        let metrics = self.metrics.clone();
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);

        metrics.on_connect();

        tracing::info!(
            peer = ?target,
            active_connections = metrics.active_connections(),
            total_connections = metrics.total_connections(),
            "⚡ New connection established"
        );

        Box::pin(async move {
            let result = inner.call(target).await;
            match &result {
                Ok(_) => {
                    metrics.on_disconnect();
                    tracing::info!(
                        active_connections = metrics.active_connections(),
                        "🔌 Connection closed"
                    );
                }
                Err(e) => {
                    metrics.on_error();
                    metrics.on_disconnect();
                    tracing::error!(
                        error = %e,
                        active_connections = metrics.active_connections(),
                        connection_errors = metrics.connection_errors(),
                        "❌ Connection error"
                    );
                }
            }
            result
        })
    }
}

/// A guard that decrements active connections when dropped.
///
/// Useful for tracking connection lifetimes in scenarios where the service
/// response outlives the initial call (e.g., long-lived streaming connections).
#[derive(Debug)]
pub struct ConnectionGuard {
    metrics: ConnectionMetrics,
    peer: Option<SocketAddr>,
}

impl ConnectionGuard {
    /// Create a new connection guard that will track a connection's lifetime.
    pub fn new(metrics: ConnectionMetrics, peer: Option<SocketAddr>) -> Self {
        metrics.on_connect();
        tracing::info!(
            peer = ?peer,
            active_connections = metrics.active_connections(),
            total_connections = metrics.total_connections(),
            "⚡ New connection established"
        );
        Self { metrics, peer }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.metrics.on_disconnect();
        tracing::info!(
            peer = ?self.peer,
            active_connections = self.metrics.active_connections(),
            "🔌 Connection closed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_metrics() {
        let metrics = ConnectionMetrics::new();
        assert_eq!(metrics.total_connections(), 0);
        assert_eq!(metrics.active_connections(), 0);
        assert_eq!(metrics.connection_errors(), 0);

        metrics.on_connect();
        assert_eq!(metrics.total_connections(), 1);
        assert_eq!(metrics.active_connections(), 1);

        metrics.on_connect();
        assert_eq!(metrics.total_connections(), 2);
        assert_eq!(metrics.active_connections(), 2);

        metrics.on_disconnect();
        assert_eq!(metrics.active_connections(), 1);

        metrics.on_error();
        assert_eq!(metrics.connection_errors(), 1);
    }

    #[test]
    fn test_connection_metrics_display() {
        let metrics = ConnectionMetrics::new();
        metrics.on_connect();
        let display = format!("{}", metrics);
        assert!(display.contains("total=1"));
        assert!(display.contains("active=1"));
        assert!(display.contains("errors=0"));
    }

    #[test]
    fn test_metrics_shared_across_clones() {
        let metrics = ConnectionMetrics::new();
        let metrics2 = metrics.clone();

        metrics.on_connect();
        assert_eq!(metrics2.active_connections(), 1);

        metrics2.on_connect();
        assert_eq!(metrics.active_connections(), 2);
    }

    #[test]
    fn test_connection_guard_drop() {
        let metrics = ConnectionMetrics::new();
        {
            let _guard = ConnectionGuard::new(metrics.clone(), None);
            assert_eq!(metrics.active_connections(), 1);
        }
        // Guard dropped — active connections should be decremented.
        assert_eq!(metrics.active_connections(), 0);
        assert_eq!(metrics.total_connections(), 1);
    }
}
