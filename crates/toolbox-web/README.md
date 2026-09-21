# toolbox-web

axum building blocks: one error shape, extractors that put a check in the
signature, and the pieces a gateway assembles.

| Module        | What it is                                                                                |
| ------------- | ----------------------------------------------------------------------------------------- |
| `error`       | the problem+json error type, with 5xx redaction                                           |
| `extract`     | request extractors: authenticated principal, validated body, page params, idempotency key |
| `health`      | the health and readiness routes                                                           |
| `client_ip`   | resolving the caller's address                                                            |
| `rate_limit`  | per-IP throttling (`rate-limit`)                                                          |
| `captcha`     | captcha verification, with a third-party and an always-pass adapter (`captcha`)           |
| `auth`        | the login routes and session middleware (`auth-router`)                                   |
| `openapi`     | spec generation and a docs page (`openapi`)                                               |
| `realtime`    | server-sent events with a fan-out hub (`realtime`)                                        |
| `idempotency` | replaying a response for a repeated key (`idempotency`)                                   |
| `pagination`  | RFC 8288 `Link` headers                                                                   |
| `server`      | the serve loop over the server crate's bind and drain                                     |
| `stack`       | applying the standard HTTP and realtime middleware stacks                                 |
| `cors`        | a loopback CORS layer                                                                     |
