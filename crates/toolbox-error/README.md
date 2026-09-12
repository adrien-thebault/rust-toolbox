# toolbox-error

The shared error vocabulary every other crate maps into: a transport-neutral
kind, a service error trait, and the RFC 9457 problem document an HTTP error
body takes. `serde` is its only dependency; `derive` adds
`#[derive(ServiceError)]`.

| Module    | What it is                                                                     |
| --------- | ------------------------------------------------------------------------------- |
| `error`   | the service-error trait and the transport-neutral error kind other crates map |
| `problem` | the RFC 9457 problem document an HTTP error body takes                        |
