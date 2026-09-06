# toolbox-server

The transport-agnostic runtime half, shared by the axum and tonic crates and
depending on neither.

| Module            | What it is                                                                |
| ----------------- | ------------------------------------------------------------------------- |
| `trace_context`   | W3C trace context: minting it, scoping it, and the request span           |
| `stack`           | the configuration the three middleware stacks share                       |
| `stack::http`     | the HTTP middleware stack                                                 |
| `stack::grpc`     | the gRPC middleware stack                                                 |
| `stack::realtime` | the middleware stack for long-lived streams                               |
| `deadline`        | request deadline propagation and enforcement                              |
| `lifecycle`       | health state, dependency probes, the graceful-drain sequence, and startup |
| `telemetry`       | log verbosity, format and filter                                          |
| `args`            | the clap server arguments                                                 |
