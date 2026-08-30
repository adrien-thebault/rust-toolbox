# toolbox-cluster

The multi-replica seam.

`event`, `kv` and `lock` each hold state across requests, so each is a trait
with a local adapter, at least one shared adapter, and capabilities declared
rather than assumed. Each is a module holding the contract, with one file per
adapter beneath it, so `ls event/` answers "what can I plug in here?". The
CloudEvents envelope rides along in `event` because `toolbox-core` takes no
dependencies and it has nowhere smaller to live.

| Module | The contract | Adapters |
|---|---|---|
| `event` | `CloudEvent` + constructors; `EventBus` and its capability set | `in_process` |
| `kv` | `KvStore`, including an **atomic** `take` | `in_memory` |
| `lock` | `LockManager`, `LockGuard` | `in_process` |
| `deployment` | `Deployment`, `Scope`, the startup guard | - |

The shared adapters live in `toolbox-cluster-postgres`, so nothing here pulls
in diesel. The clock lives in `toolbox-schedule`, its only consumer: it is a
determinism seam, not a replication one.

## The guard

`check_deployment` runs at startup and has two severities, because not every
local adapter is equally wrong. An in-process event bus under three replicas
means a subscriber never sees two thirds of the events, so it refuses to start.
Per-process rate limiting under three replicas means three times the allowance,
so it warns. Refusing to boot for the second kind is how a guard becomes
something people switch off.

`KvStore::take` is atomic because refresh-token rotation is built on it, and a
get-then-delete race silently permits exactly the replay rotation exists to
catch.
