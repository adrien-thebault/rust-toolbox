# toolbox

The facade.

One dependency line and one version to bump.

```toml
toolbox = { git = "...", tag = "v0.4.1", features = ["db", "web"] }
```

| Module | What it holds |
|---|---|
| `deps` | the upstream crates whose types cross the boundary |
| `prelude` | the handful of names almost every file needs |

Each feature pulls in the crate of the same name, and enabling `web` also
enables `server`, `cluster`, `auth` and `core`.

`deps` re-exports `axum`, `tonic`, `http`, `tower`, `tower-http`. It
deliberately does not re-export `diesel`, `serde` or `prost`: their derives emit
absolute paths that only resolve when the crate is a direct dependency under
that exact name.
