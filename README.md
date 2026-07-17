# rust-toolbox

Generic, domain-agnostic Rust building blocks, factored out of a service
backend so they can be reused across projects instead of copy-pasted. Every
module is behind its own feature flag and has zero knowledge of any
particular project's domain.

## Using it

Not published to crates.io - pull it in as a git dependency:

```toml
[dependencies]
rust-toolbox = { git = "https://github.com/adrien-thebault/rust-toolbox.git", branch = "master", default-features = false, features = ["sqlite", "tower", "tonic"] }
```

Enable only the features you need - each one pulls in its own dependency
set (diesel, axum, tonic, lettre, ...).

## Development

```sh
cargo build
cargo fmt
cargo clippy --all-targets --features sqlite,axum,mail
./scripts/test-all.sh   # runs the suite once per feature combination below
```

`sqlite`/`mysql`/`postgresql` are mutually exclusive diesel backends, so
there's no single "test everything" feature set (`--all-features` fails to
compile) - `scripts/test-all.sh` runs `cargo test` once per combination
(`axum,mail,tower`; `sqlite`; `postgresql`; `mysql`) instead.

The `tests/` directory is a single harness (`tests/integration.rs`) whose
module tree mirrors `src/` - `tests/diesel_tools/repository/find.rs` tests
`src/diesel_tools/repository/find.rs`, and so on.

See [CONTRIBUTING.md](CONTRIBUTING.md) for commit conventions (this repo
follows [Conventional Commits](https://www.conventionalcommits.org/)) and
how the changelog is generated.

## License

[MIT](LICENSE)
