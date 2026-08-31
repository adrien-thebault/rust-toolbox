# toolbox-grpc

tonic building blocks, split by direction: `client` calls another service,
`server` serves one.

| Module | What it holds |
|---|---|
| `client` | `client()` and one channel type; `uri` points at your LB or `Service` |
| `client::interceptor` | what every outgoing request carries: deadline, shared secret, forwarded principal |
| `client::retry` | `RetryPolicy` and `with_retry` |
| `client::error` | `ClientError` |
| `server` | `serve()`, health and reflection |
| `server::shared_secret` | `shared_secret_layer`: the "is this an allowed caller" gate |
| `server::identity` | `identity_layer`, `require`/`optional`: who the end user is |
| `status` | `to_status`, `from_status`, and the `ErrorKind` mapping |
| `pagination` | the shared pagination messages |
| `limits` | `MessageLimits`, the one value both ends read |

`GrpcResult<T>` is a plain type alias. There is deliberately no blanket
`impl<E: ServiceError> From<E> for tonic::Status` - `Status` is foreign and `E`
is a type parameter, so it violates the orphan rule here exactly as it would in
any consumer. Each consumer keeps its own one-line `From`, which is legal.

`to_status` replaces the message on `Internal`: a gRPC message crosses to
whoever called, and an internal failure's `Display` is dependency text.

Client-side discovery is deliberately absent. `client()` connects lazily to one
`uri` - point it at a load balancer, a mesh, or a Kubernetes `Service`.
Re-resolving DNS in-process to spread load across a headless service is a
workaround for a missing proxy, better solved by adding one.

The `x-shared-secret` gate (`shared_secret_layer`) and identity resolution
(`identity_layer`) are two layers, not one: the first answers "is this an
allowed caller", the second "and who is the end user".

`identity_layer` extracts nothing by itself - you compose its credential
sources and it runs whatever they find through a `ProviderRegistry`:

```rust
identity_layer(registry)
    .extracting(identity::forwarded_principal) // the gateway's x-fwd-principal
    .extracting(identity::bearer)              // a direct Authorization: Bearer
    .extracting(|headers| ...)                 // whatever else your registry knows
```
