# incubator

Crates that live in this repository but are not part of the toolbox.

Each one carries its own `[workspace]` table, so the toolbox does not build,
test or lint it. They depend on the toolbox by relative path, exactly as an
outside consumer would - which is the point: it is the rehearsal for leaving.

## Why anything is here

The toolbox is generic building blocks. A crate lands here when it turns out to
have a **domain** - when its types encode what something *is* rather than how to
plumb it - but the code is worth keeping while it finds a home.

Code that is not compiled rots. `.github/workflows/incubator.yml` type-checks
every crate here on each push, which is cheap and keeps the rot visible.

## toolbox-files

Upload policy, streaming ingest, cache-correct serving, a mountable gRPC file
service, and the multipart-to-gRPC adapters.

It is here because a file, in this crate, has an owner, a quota, a declared
type and a thumbnail: that is an application's model, not a building block. And
it was carrying an architectural cost the rest of the toolbox paid for it -
`toolbox-web` had a `grpc` feature whose *only* purpose was the three adapter
functions in `src/web.rs`, which is to say the single exception to the rule
that `toolbox-web` never depends on `toolbox-grpc` existed for this crate
alone. Moving it out removed the exception.

What has to happen before it leaves: pick whether it becomes its own repository
or folds into the one consumer that needs it, and decide whether `FileMeta` and
`UploadPolicy` are the right model or an artefact of the first project that
needed them.
