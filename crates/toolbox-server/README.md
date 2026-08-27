# toolbox-server

The runtime half, for both transports.

Shared by axum and tonic; depends on neither.

| Module | What it holds |
|---|---|
| `trace_context` | W3C `traceparent`, minted when absent, with `x-request-id` as an alias |
| `trace_context::layer` | the tower layer that scopes it for the whole request |
| `stack` | `StackConfig`, shared by the three below |
| `stack::http` | `http_stack`: catch-panic, trace context, span, deadline |
| `stack::grpc` | `grpc_stack`: the same, classified by `grpc-status` |
| `stack::realtime` | `realtime_stack`: no timeout, no body limit, on purpose |
| `deadline` | the `DEADLINE` task-local, and the gRPC timeout format |
| `deadline::layer` | the tower layer that enforces it |
| `shutdown` | the five-step drain |
| `telemetry` | `-v`/`-q`, `LOG_FORMAT`, `RUST_LOG` |
| `serve` | the deployment check and the bind |
| `args` | `ServerArgs` and `DeploymentArgs` |
| `span` | the request span |

`realtime_stack` has no timeout and no body limit, and that is the entire
reason it exists: a 30-second request timeout kills every SSE and WebSocket
connection in production while working perfectly against a local client that
reconnects instantly.

The drain waits between failing readiness and closing the listener. That step
is the one that stops a rolling deploy dropping requests.
