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

## Sign your commits

Commits should be signed. The simplest option (git ≥ 2.34) is SSH-based
signing, reusing a key you likely already have:

```sh
git config user.signingkey ~/.ssh/id_ed25519.pub   # or whichever key you use
git config gpg.format ssh
git config commit.gpgsign true
```

GitHub will show a "Verified" badge once your public key is also added under
[github.com/settings/keys](https://github.com/settings/keys) as a **signing
key** (separate from an authentication key, even if it's the same keypair).

Prefer GPG instead? `git config gpg.format openpgp` (the default) with
`user.signingkey <your-gpg-key-id>` and the same `commit.gpgsign true` works
the same way.

`commit.gpgsign true` above is set globally for this repo via local git
config (not committed) - there's no way to *require* signing from files
alone. To actually enforce it server-side, enable "Require signed commits"
in the repo's branch protection settings on GitHub.

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
