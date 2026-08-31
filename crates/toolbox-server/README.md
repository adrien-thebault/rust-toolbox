# toolbox-server

The runtime half, for both transports.

Shared by axum and tonic; depends on neither.

| Module | What it holds |
|---|---|
| `trace_context` | W3C `traceparent` (minted when absent, `x-request-id` alias), the layer that scopes it, and the request span |
| `stack` | `StackConfig`, shared by the three below |
| `stack::http` | `http_stack`: catch-panic, trace context, span, deadline |
| `stack::grpc` | `grpc_stack`: the same, classified by `grpc-status` |
| `stack::realtime` | `realtime_stack`: no timeout, no body limit, on purpose |
| `deadline` | the `DEADLINE` task-local, the gRPC timeout format, and the layer that enforces it |
| `shutdown` | the five-step drain |
| `startup` | the deployment check and the bind |
| `telemetry` | `-v`/`-q`, `LOG_FORMAT`, `RUST_LOG` |
| `args` | `ServerArgs` and `DeploymentArgs` |

`realtime_stack` has no timeout and no body limit, and that is the entire
reason it exists: a 30-second request timeout kills every SSE and WebSocket
connection in production while working perfectly against a local client that
reconnects instantly.

The drain waits between failing readiness and closing the listener. That step
is the one that stops a rolling deploy dropping requests.
