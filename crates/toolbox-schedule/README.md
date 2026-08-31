# toolbox-schedule

Cluster-safe scheduled tasks.

| Module | What it holds |
|---|---|
| `scheduler` | `Scheduler`, `SchedulerBuilder`, `tick_once`, `run_now` |
| `trigger` | `Trigger`, and the only place `croner` is named |
| `job` | `Job`, `RunMode`, `Overlap`, `JobOutcome`, `JobSummary` |
| `clock` | `Clock`, with `system` and `manual` adapters |
| `error` | `ScheduleError` |

Three defaults that are otherwise chosen by accident: `Exclusive` rather than
every-replica, `Overlap::Skip` on an overrun, and a **mandatory** timeout.

A scheduler is not a job queue. This is the clock; a queue is a different
thing, and they compose - "every night at 3am, email 500 people" is one
occurrence here that enqueues 500 jobs there.

Its own crate because cargo unifies features across a workspace: a `schedule`
feature on `toolbox-cluster` would compile croner and the metrics facade into
every gateway, including ones that schedule nothing.

**Cron expressions are UTC.** Named timezones are deliberately not supported: a
wall-clock schedule needs a policy for the spring-forward hour that does not
exist and the autumn hour that happens twice, and no policy is the one every
caller expected. A job pinned to a local hour drifts by an hour twice a year,
and that is the price.

An exclusive job's lease covers the window until the **next** occurrence, not
just the run: releasing when the work finished would let the next replica to
tick redo the same occurrence.
