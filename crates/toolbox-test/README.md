# toolbox-test

The test harness: throwaway databases, an in-process gateway, and a
problem-document assertion. Dev-only.

| Module    | What it is                                  |
| --------- | ------------------------------------------- |
| `db`      | a private, migrated, self-deleting database |
| `gateway` | a gateway driven in process, with no port   |
| `cluster` | gRPC backends on ephemeral ports            |
| `problem` | the problem-document assertion macro        |
