# toolbox-grpc

tonic building blocks.

| Module | What it holds |
|---|---|
| `status` | `to_status`, `from_status`, and the `ErrorKind` mapping |
| `backend` | `backend()` and one channel type whatever the discovery |
| `backend::interceptor` | what every outgoing request carries: deadline and credential |
| `backend::dns` | the channel that re-resolves and rebalances |
| `discovery` | static, DNS with re-resolution, or a proxy |
| `retry` | `RetryPolicy` and `with_retry` |
| `auth` | service-to-service credentials |
| `pagination` | the shared pagination messages |
| `file` | the file-transfer wire contract |
| `serve` | `serve_grpc`, health and reflection |

`GrpcResult<T>` is a plain type alias. There is deliberately no blanket
`impl<E: ServiceError> From<E> for tonic::Status` - `Status` is foreign and `E`
is a type parameter, so it violates the orphan rule here exactly as it would in
any consumer. Each consumer keeps its own one-line `From`, which is legal.

`to_status` replaces the message on `Internal`: a gRPC message crosses to
whoever called, and an internal failure's `Display` is dependency text.

`Discovery::Dns` re-resolves. `connect_lazy` on a DNS name opens one connection
to whichever address resolved first and never looks again, so a scale-out
changes nothing - a failure that looks like a successful deploy.
