# toolbox-schedule

Cluster-safe scheduled tasks, exclusive by default, and the clock port they
read time from.

| Module      | What it is                                             |
| ----------- | ------------------------------------------------------ |
| `scheduler` | the scheduler, its builder, and the concurrency bound  |
| `trigger`   | UTC cron, fixed-delay, and fixed-rate triggers         |
| `job`       | jobs, outcomes, and their run-mode and overlap options |
| `clock`     | the clock port, with system and manual adapters        |
| `error`     | the schedule error type                                |
