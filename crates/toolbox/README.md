# toolbox

The facade: one dependency line and one version, with a feature per crate.

The default features are `error` and `pagination`. Larger features pull in
their prerequisites: `db` includes both defaults, `server` includes `cluster`,
`auth` includes password authentication, and the transport features include
the shared layers they need. `full` enables `db`, `web`, `grpc`, and `schedule`.

| Module    | What it is                                             |
| --------- | ------------------------------------------------------ |
| `deps`    | the upstream crates whose types cross the API boundary |
| `prelude` | the names almost every file needs                      |

Each enabled crate is also available under its matching module, such as
`toolbox::db`, `toolbox::web`, or `toolbox::grpc`.
