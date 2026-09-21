# toolbox-server

The transport-agnostic runtime half, shared by the axum and tonic crates and
depending on neither.

| Module              | What it is                                                       |
| ------------------- | ---------------------------------------------------------------- |
| `trace_context`     | W3C trace context: minting, scoping, and request spans           |
| `stack`             | the configuration the three middleware stacks share              |
| `stack::http`       | the HTTP middleware stack                                        |
| `stack::grpc`       | the gRPC middleware stack                                        |
| `stack::realtime`   | the middleware stack for long-lived streams                      |
| `deadline`          | request deadline propagation and enforcement                     |
| `server`            | binding, background tasks, dependency probes, and graceful drain |
| `server::health`    | health checks and polling probes                                 |
| `server::lifecycle` | the combined readiness and shutdown state                        |
| `server::shutdown`  | shutdown signaling and drain timing                              |
| `telemetry`         | log verbosity, format, and filter                                |
| `args`              | the clap server arguments (`clap`)                               |
