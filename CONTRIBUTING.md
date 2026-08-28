# Contributing

## Before adding anything to the toolbox

Three questions, in order. They exist because it is the modules that were
merely "nicer than calling the thing directly" that rot.

### Does a crate already do this?

Search crates.io and lib.rs. Read the top two results' **docs**, not their
names. Then name which rule your module satisfies:

1. it unifies an error type across a boundary;
2. it encodes a decision you would otherwise re-make wrong;
3. it removes a trap the underlying crate makes easy;
4. it bridges two crates that do not know about each other;
5. it is invoked identically in every project **and** the underlying API needs
   more than ten lines of setup.

If the answer is "it is "nicer than calling X directly" - stop.
`mail_tools`, and it is why the mail transport wrapper is gone while the
templates stayed.

### Does a standard already define this?

For any format, header, envelope, protocol or wire contract. If a standard
exists and fits, implement it even when bespoke would be 20% less code: RFC
9457 for errors, CloudEvents for events, W3C Trace Context for request ids,
the IETF `Idempotency-Key` and `RateLimit` names. If a standard exists and
does **not** fit, write why in the doc comment.

### Does it hold state across requests?

Then it is a **trait with adapters**, not a struct: a local adapter, at least
one shared adapter, capabilities declared rather than assumed, and unsupported
operations failing at *wiring* time rather than at runtime. And it declares a
`Scope` so the deployment guard can refuse to start the process.

### Then write the answer down

**One sentence at the top of the module**, right after the summary line,
saying why the module is there. State the reason directly; no "Why this
exists:" preamble. That sentence is what lets someone re-open the decision in
two years when the ecosystem has moved. A module without one is a module
nobody can delete with confidence.

Anything larger than a sentence goes in the crate's `README.md`, next to the
table of what its modules do.

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

`core`, `macros`, `db`, `cluster`, `cluster-postgres`, `schedule`, `server`,
`auth`, `web`, `grpc`, `files`, `test`, `toolbox`, `template`, `examples`.

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

Every breaking release ships a migration guide: ordered steps, literal
find-and-replace tables, explicit deletions, a verification command that must
pass, and a `rg` check that must return nothing. It is handed to whoever is
migrating rather than committed here, because it names a specific consumer's
files and will be wrong within a release.

## Architecture decisions

Anything that was a close call gets written into the doc comment of whatever it
decided, including **the signal that would reopen it**. That last part is the
point: it is the difference between a decision and a habit.

## Working documents

Notes for one piece of work - a review, a rewrite plan, a migration guide - are
kept out of the repository. They name specific consumers and go stale
immediately. Anything worth keeping belongs in a crate README or a doc comment
instead.
