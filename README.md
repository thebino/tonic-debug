# tonic-debug

Developer-focused debugging utilities for gRPC services built with `tonic`.

This crate aims to make gRPC issues easier to investigate by providing a composable `tower::Layer` ("DebugLayer") that can:

- Log gRPC method, metadata, and status codes
- Render protobuf request/response bodies in human-readable formats (e.g. pretty JSON) using reflection/descriptor sets
- Limit logged body size to avoid noisy logs (`max_body_log_bytes`)
- Integrate cleanly with `tracing` (and optionally OpenTelemetry via feature flags)

## Status

Early PoC (work in progress). APIs may change.

## Non-goals (for now)

- Wire-level HTTP/2 frame dumps
- Full request capture/replay tooling
- Debug UI/dashboard

## Planned usage (sketch)

```rust
use tonic_debug::{DebugLayer, Format};

Server::builder()
    .layer(DebugLayer::new()
        .format(Format::PrettyJson)
        .max_body_log_bytes(4096))
    .add_service(my_service)
    .serve(addr)
    .await?;
```

## License

TBD
