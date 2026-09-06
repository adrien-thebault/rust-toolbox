# toolbox-web

axum building blocks: one error shape, extractors that put a check in the
signature, and the pieces a gateway assembles.

| Module        | What it is                                                                                |
| ------------- | ----------------------------------------------------------------------------------------- |
| `error`       | the problem+json error type, with 5xx redaction                                           |
| `extract`     | request extractors: authenticated principal, validated body, page params, idempotency key |
| `health`      | the health and readiness routes                                                           |
| `client_ip`   | resolving the caller's address                                                            |
| `rate_limit`  | per-IP throttling                                                                         |
| `captcha`     | captcha verification, with a third-party and an always-pass adapter                       |
| `auth`        | the login routes and session middleware                                                   |
| `openapi`     | spec generation and a docs page                                                           |
| `realtime`    | server-sent events with a fan-out hub                                                     |
| `idempotency` | replaying a response for a repeated key                                                   |
| `pagination`  | RFC 8288 `Link` headers                                                                   |
| `server`      | the serve loop over the server crate's bind and drain                                     |
| `cors`        | a loopback CORS layer                                                                     |
