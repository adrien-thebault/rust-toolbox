# toolbox-test

The test harness.

Dev-only. Nothing here should ever be a runtime dependency.

| Module | What it holds |
|---|---|
| `db` | `temp_db`: a private, migrated, self-deleting database |
| `app` | `TestApp`: a router driven in process, no port and no readiness wait |
| `cluster` | `TestCluster`: gRPC backends on ephemeral ports |
| `problem` | `assert_problem!` |

`assert_problem!` checks the media type as well as the status and code: a body
that is JSON but not problem+json is exactly the bug the error shape exists to
prevent, and a test that only checked the code would not see it.

The readiness wait is bounded and names the backend that failed.
