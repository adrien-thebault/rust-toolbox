# toolbox-grpc

tonic building blocks, split by direction: a client that calls another
service, a server that serves one.

| Module                  | What it is                                                                     |
| ----------------------- | ------------------------------------------------------------------------------ |
| `client`                | connecting to another service and health-polling it                            |
| `client::interceptor`   | what every outgoing request carries: deadline, trace context, secret, identity |
| `client::retry`         | the retry policy                                                               |
| `client::error`         | the client error type                                                          |
| `server`                | the serve loop with the standard stack, drain, health and reflection           |
| `server::shared_secret` | the caller-authorization layer                                                 |
| `server::identity`      | the end-user identity layer                                                    |
| `status`                | conversion between the service-error trait and a tonic status                  |
| `pagination`            | the shared pagination messages and helpers                                     |
| `limits`                | the message size limits both ends read                                         |
