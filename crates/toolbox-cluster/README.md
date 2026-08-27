# toolbox-cluster

The multi-replica seam.

Everything here holds state across requests, so each one is a trait with a
local adapter, a shared adapter, and capabilities declared rather than assumed.
Each is a module holding the contract, with one file per adapter beneath it, so
`ls bus/` answers "what can I plug in here?".

| Module | The contract | Adapters |
|---|---|---|
| `bus` | `EventBus`, the capability set | `in_process`, `null` |
| `key_value` | `KeyValueStore`, including an **atomic** `take` | `in_memory` |
| `lock` | `LockManager`, `LockGuard` | `in_process` |
| `clock` | `Clock` | `system`, `manual` |
| `event` | CloudEvents 1.0, from the official SDK | - |
| `deployment` | `Deployment`, `Scope`, the startup guard | - |

The shared adapters live in `toolbox-cluster-postgres`, so nothing here pulls
in diesel.

## The guard

`check_deployment` runs at startup and has two severities, because not every
local adapter is equally wrong. An in-process event bus under three replicas
means a subscriber never sees two thirds of the events, so it refuses to start.
Per-process rate limiting under three replicas means three times the allowance,
so it warns. Refusing to boot for the second kind is how a guard becomes
something people switch off.

`KeyValueStore::take` is atomic because refresh-token rotation is built on it,
and a get-then-delete race silently permits exactly the replay rotation exists
to catch.
