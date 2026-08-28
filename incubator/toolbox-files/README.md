# toolbox-files

Files: policy, ingest, serving.

| Module | What it holds |
|---|---|
| `policy` | `UploadPolicy`, `MimePolicy`, `Quota` |
| `ingest` | one pass: source to hasher to store, with the cap enforced as bytes flow |
| `serve` | `ServeDecision` - status, headers, resolved range |
| `meta` | `FileMeta` and the content-addressed key |
| `stream` | reading a stored file back |
| `service` | a ready-made gRPC service (feature `service`) |
| `service::hooks` | what a consumer plugs in: who may act, and what to do afterwards |

There is no `FileStore` trait: `object_store::ObjectStore` already is one, with
local, S3, GCS, Azure and in-memory backends maintained by somebody else.

Four decisions encoded here so they are not re-made per project: keys are the
blake3 of the content, so identical uploads deduplicate and the URL is
immutable; the media type is **sniffed**, never taken from the filename; every
download carries `Content-Security-Policy: sandbox` and `nosniff`; and the size
cap bites at the byte that crosses it.

`serve()` returns a decision rather than a response, so a gRPC service can use
the same logic for its metadata.

The `service` feature is the ready-made half. Its persistence is a
`FileRecords` trait with a macro you expand in your own crate, because the
`Entity` derive needs a concrete backend and only you know which one.
