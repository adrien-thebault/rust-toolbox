# toolbox-cluster

The traits a multi-replica deployment coordinates through, each with an
in-memory adapter and room for a shared one.

| Module  | What it is                                          |
| ------- | --------------------------------------------------- |
| `event` | the event-bus contract and the CloudEvents envelope |
| `kv`    | the key-value store contract                        |
| `lock`  | the lock manager contract                           |
