# toolbox-db

A diesel connection pool that keeps blocking calls off the async runtime,
generic over the backend.

| Module       | What it is                                                                                    |
| ------------ | --------------------------------------------------------------------------------------------- |
| `db`         | the pool and the `run`/`query`/`transaction` wrappers that move diesel onto a blocking thread |
| `entity`     | the trait the `Entity` derive implements                                                      |
| `pagination` | window-function pagination that composes onto any diesel query, and the sort-field check      |
| `migrate`    | migrations, serialised across replicas by a session lock                                      |
| `sqlite`     | connection pragmas                                                                            |
| `args`       | the clap arguments for the pool                                                               |
