# toolbox-core

The vocabulary every other crate speaks.

Errors, problems and pagination, with `serde` and `thiserror` as its only
dependencies. Anything may depend on this without inheriting anything.

| Module | What it holds |
|---|---|
| `error` | `ErrorKind`, `ErrorInfo`, and the `ServiceError` trait a domain error implements |
| `problem` | RFC 9457 `Problem`, the shape every HTTP error body takes |
| `page` | `Page` and `PageRequest` - one representation that serves query strings, protobuf and SQL |
| `page::sort` | `Sort`, `SortItem`, `SortDirection`, validated against an allowlist |

`ErrorKind` is transport-neutral on purpose: `toolbox-grpc` maps it to a
`tonic::Code` and `toolbox-web` to an HTTP status, and neither mapping lives
here. That is what keeps this crate free of both.

`PageRequest`'s fields are private and its constructors validate, so an invalid
window cannot be built and nothing downstream needs a defensive clamp.
`Sort::validate` is here too, because rejecting an unknown sort field is what
stands between a query parameter and SQL injection.
