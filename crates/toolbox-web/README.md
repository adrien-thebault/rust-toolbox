# toolbox-web

axum building blocks.

| Module | What it holds |
|---|---|
| `error` | `ApiError`: problem+json, 5xx redaction, `Retry-After` |
| `extract` | `Authenticated<R>`, `ValidJson`, `PageParams`, `Idempotent` |
| `health` | `/health` and `/ready` |
| `client_ip` | one answer to "who is the caller", shared by every subsystem |
| `rate_limit` | per-IP throttling (feature `rate-limit`) |
| `captcha` | `CaptchaVerifier`, with `third_party` and `always_pass` beneath it (feature `captcha`) |
| `auth` | the login routes and the session middleware, in `routes`/`session`/`forwarded`/`limiter` (feature `auth-router`) |
| `openapi` | spec generation with a stable key order (feature `openapi`) |
| `realtime` | SSE with a fan-out hub and ticket auth (feature `realtime`) |
| `idempotency` | replaying a response for a repeated key (feature `idempotency`) |
| `pagination` | RFC 8288 `Link` headers |
| `server` | `serve`: the axum serve loop over `toolbox-server`'s bind and drain |
| `cors` | one function |

A 5xx clears `detail` and `metadata` before serializing, because both are built
from the error's own `Display` - which for an internal failure is database or
dependency text. The stable code survives, so support can still act on it.

`Authenticated<Admin>` in a signature cannot be forgotten, is visible in the
route table, and is checked by the type system.

Mount `health_router` **outside** the traced stack: inside it, the 503 that
`/ready` returns while draining is logged at ERROR on every rolling deploy.
