# toolbox-test

The test harness: throwaway databases, an in-process gateway, a gRPC cluster,
and a problem-document assertion. Dev-only.

| Module    | What it is                                                          |
| --------- | ------------------------------------------------------------------- |
| `db`      | private, optionally migrated, self-deleting SQLite databases (`db`) |
| `gateway` | a gateway driven in process, with no port (`web`)                   |
| `cluster` | gRPC backends on ephemeral ports (`grpc`)                           |
| `problem` | the problem-document assertion macro                                |
