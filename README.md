# tonic-debug

![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/thebino/tonic-debug/ci.yaml?style=for-the-badge)
[![License](https://img.shields.io/github/license/thebino/tonic-debug?style=for-the-badge)](./LICENSE-APACHE.md)
A debugging and diagnostics middleware for [tonic](https://github.com/hyperium/tonic) gRPC servers.

## Problem

Debugging gRPC services built with tonic is hard:

- Messages are binary-encoded protobufs — not human-readable
- Connection issues between clients and servers are opaque
- Standard logging lacks the detail needed to quickly diagnose problems

## Solution

`tonic-debug` is a Tower middleware that slots into your tonic server and provides:

- **Human-readable protobuf inspection** — decodes protobuf wire format without needing `.proto` schemas, showing field numbers, wire types, and values
- **Connection lifecycle observability** — tracks connection events (connect, disconnect, errors) with active/total counters via tower/hyper integration
- **Structured logging via `tracing`** — all output goes through the [`tracing`](https://docs.rs/tracing) crate with structured fields for easy filtering and aggregation
- **Optional OpenTelemetry export** — enable the `opentelemetry` feature flag to forward spans and attributes to your OTel collector

## Quick Start

Add `tonic-debug` to your `Cargo.toml`:

```toml
[dependencies]
tonic-debug = "0.1"
```

Then add the `DebugLayer` to your tonic server:

```rust,no_run
use tonic_debug::DebugLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing (e.g. with tracing-subscriber)
    tracing_subscriber::fmt()
        .with_env_filter("tonic_debug=debug")
        .init();

    tonic::transport::Server::builder()
        .layer(DebugLayer::new())
        .add_service(/* your gRPC service */)
        .serve("0.0.0.0:50051".parse()?)
        .await?;

    Ok(())
}
```

## Configuration

Fine-tune what gets logged using the builder API or `DebugConfig`:

```rust
use tonic_debug::{DebugLayer, DebugConfig};

// Builder API
let layer = DebugLayer::new()
    .log_headers(true)
    .log_bodies(true)
    .log_response_frames(true)
    .max_body_bytes(8192)
    .hex_dump(false);

// Or use DebugConfig directly
let layer = DebugLayer::with_config(DebugConfig {
    log_headers: true,
    log_bodies: true,
    log_response_frames: true,
    max_body_bytes: 8192,
    hex_dump: false,
});
```

| Option               | Default | Description                                        |
|----------------------|---------|----------------------------------------------------|
| `log_headers`        | `true`  | Log request/response headers and custom metadata   |
| `log_bodies`         | `true`  | Log protobuf message contents                      |
| `log_response_frames`| `true`  | Log individual response body frames as they stream |
| `max_body_bytes`     | `4096`  | Maximum bytes to capture for body inspection       |
| `hex_dump`           | `false` | Include hex dump of raw bytes in output            |

## Connection Tracking

Track client connection lifecycle with `ConnectionTrackerLayer`:

```rust
use tonic_debug::{ConnectionTrackerLayer, ConnectionMetrics};

let metrics = ConnectionMetrics::new();
let connection_layer = ConnectionTrackerLayer::with_metrics(metrics.clone());

// Query metrics at any time
println!("Active: {}", metrics.active_connections());
println!("Total: {}", metrics.total_connections());
println!("Errors: {}", metrics.connection_errors());
```

## Protobuf Inspection

The crate decodes raw protobuf wire format without schemas, producing output like:

```text
gRPC frame (compressed=false, 15 bytes)
  field 1 (varint): 42
  field 2 (length-delimited): "hello world"
```

You can also use the inspection utilities directly:

```rust
use tonic_debug::inspect;

let data = &[0x08, 0x96, 0x01]; // field 1, varint 150
if let Some(fields) = inspect::decode_protobuf(data) {
    for field in &fields {
        println!("{}", field);
    }
}
```

## Feature Flags

| Feature         | Description                                               |
|-----------------|-----------------------------------------------------------|
| `opentelemetry` | Adds OpenTelemetry semantic convention attributes and     |
|                 | span enrichment via `tracing-opentelemetry`               |

Enable with:

```toml
[dependencies]
tonic-debug = { version = "0.1", features = ["opentelemetry"] }
```

## Log Levels

`tonic-debug` uses the following `tracing` levels:

| Level   | What gets logged                                          |
|---------|-----------------------------------------------------------|
| `INFO`  | Request/response summary, connection events               |
| `WARN`  | Non-OK gRPC status codes                                  |
| `ERROR` | Connection errors, response body errors                   |
| `DEBUG` | Headers, custom metadata, response frames, protobuf fields|
| `TRACE` | Stream completion summaries                               |



## 🏛️ License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

## 🧩 Contribution

This is a free and open project and lives from contributions of the community.

See our [Contribution Guide](CONTRIBUTING.md)
