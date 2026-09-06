# toolbox-schedule

Scheduled tasks that run once per cluster, and the clock port they read time
from.

| Module      | What it is                                            |
| ----------- | ----------------------------------------------------- |
| `scheduler` | the scheduler and its builder                         |
| `trigger`   | cron and fixed-interval triggers                      |
| `job`       | a job definition and its run-mode and overlap options |
| `clock`     | the clock port, with system and manual adapters       |
| `error`     | the schedule error type                               |
