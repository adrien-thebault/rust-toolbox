# toolbox-cluster

The multi-replica seam.

`event`, `kv` and `lock` each hold state across requests, so each is a trait
with a local adapter and at least one shared adapter. Each is a module holding
the contract, with one file per adapter beneath it, so `ls event/` answers
"what can I plug in here?". The CloudEvents envelope rides along in `event`
because `toolbox-core` takes no dependencies and it has nowhere smaller to
live.

| Module | The contract | Adapters |
|---|---|---|
| `event` | `CloudEvent` + constructors; `EventBus` | `in_process` |
| `kv` | `KvStore`, including an **atomic** `take` | `in_memory` |
| `lock` | `LockManager`, `LockGuard` | `in_process` |

The clock lives in `toolbox-schedule`, its only consumer: it is a determinism
seam, not a replication one.

`KvStore::take` is atomic because refresh-token rotation is built on it, and a
get-then-delete race silently permits exactly the replay rotation exists to
catch.
