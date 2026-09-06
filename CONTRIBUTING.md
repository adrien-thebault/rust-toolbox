# Contributing

## Commit messages: Conventional Commits

Every commit message must follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

Allowed types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`,
`chore`, `ci`, `build`, `revert`.

**Scope names the crate**, without the `toolbox-` prefix:

`core`, `macros`, `db`, `cluster`, `schedule`, `server`, `auth`, `web`, `grpc`,
`test`, `toolbox`, `template`, `examples`.

`committed` does not enforce the scope list, so it is a convention this file
holds rather than a check. It does enforce the type, the subject length and
the format.

A `!` after the type/scope (`feat!:`, `refactor(db)!:`) or a
`BREAKING CHANGE:` footer marks a breaking change.

This matters beyond style: `CHANGELOG.md` is generated from these messages, so
a message that does not parse is either dropped from the changelog or
miscategorized.

### Optional: enforce it locally with a commit-msg hook

```sh
cargo install committed
git config core.hooksPath .githooks
```

Configuration lives in `committed.toml`. Subjects are lowercase here - the
changelog capitalizes them on render, so both stay consistent.

## Changelog

`CHANGELOG.md` is generated from git history by
[git-cliff](https://git-cliff.org/), driven entirely by Conventional Commits.
No manual editing.

```sh
cargo install git-cliff
./scripts/changelog.sh                # regenerate everything
./scripts/changelog.sh --unreleased   # just what has changed since the last tag
```

Grouping and formatting live in `cliff.toml`. Tag a release before
regenerating, so git-cliff can attribute commits to it.

## Releases

`release-plz` opens the release PR; merging it bumps the one workspace
version, regenerates the changelog and creates a git release.

**Nothing is published to a registry.** Every crate carries `publish = false`,
and that is not an oversight. Consumers depend on git and pin a tag.
