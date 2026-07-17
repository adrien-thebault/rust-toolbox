# Contributing

## Commit messages: Conventional Commits

Every commit message must follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

Allowed types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`,
`chore`, `ci`, `build`, `revert`. Scope is optional and free-form (e.g.
`diesel_tools`, `axum_tools`) but should name the module the change touches
when there is one. A `!` after the type/scope (`feat!:` or
`feat(axum_tools)!:`) or a `BREAKING CHANGE:` footer marks a breaking change.

This matters beyond style: `CHANGELOG.md` is generated from these messages
(see below), and a message that doesn't parse either gets dropped from the
changelog or miscategorized.

### Optional: enforce it locally with a commit-msg hook

This repo ships a `commit-msg` hook that rejects non-conforming messages
before the commit is even created, using
[`committed`](https://github.com/crate-ci/committed) (a Rust
Conventional-Commits linter - no custom validation script involved). It's
opt-in, not installed by default:

```sh
cargo install committed
git config core.hooksPath .githooks
```

Configuration lives in `committed.toml`.

## Changelog

`CHANGELOG.md` is generated from git history by
[git-cliff](https://git-cliff.org/), driven entirely by Conventional Commits

- no manual changelog editing.

```sh
cargo install git-cliff
./scripts/changelog.sh          # regenerate CHANGELOG.md for everything
./scripts/changelog.sh --unreleased   # just what's changed since the last tag
```

Grouping/formatting rules live in `cliff.toml`. Tag a release (`git tag
vX.Y.Z`) before regenerating so git-cliff can attribute commits to it.
