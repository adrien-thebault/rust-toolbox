# toolbox-core

The shared vocabulary every other crate depends on: error, problem and
pagination types. `serde` and `thiserror` are its only dependencies.

| Module       | What it is                                                                    |
| ------------ | ----------------------------------------------------------------------------- |
| `error`      | the service-error trait and the transport-neutral error kind other crates map |
| `problem`    | the RFC 9457 problem document an HTTP error body takes                        |
| `page`       | one pagination representation for query strings, protobuf and SQL             |
| `page::sort` | sort fields, and their validation against an allowlist                        |
