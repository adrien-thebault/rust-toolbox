# toolbox-auth

Identity, with no transport dependency.

A backend validating a token should not have to compile axum - or the cluster
traits - to do it, which is why this is its own crate. It depends only on
`toolbox-core`.

| Module | What it holds |
|---|---|
| `principal` | `Principal`, the `Role` trait, `AuthError` |
| `principal::mapping` | `PrincipalMapping`, `MappingPath` - where identity lives in a provider's token |
| `provider` | `IdentityProvider` (`id` + `authenticate`), `ProviderRegistry`, `Credential` |
| `provider::password` | `PasswordIdentityProvider`, `UserStore`, `auth_epoch` (feature `password`) |
| `provider::proxy_header` | `ForwardedIdentityProvider`, `ForwardedHeaders` - trusting an authenticating reverse proxy |
| `provider::jwt` | `JwtIdentityProvider`, `Claims` - mint the gateway's sessions, verify a bearer token |

Every name is re-exported flat, so `toolbox_auth::PasswordIdentityProvider`
works.

`ProviderRegistry` is the single place a `Credential` becomes a `Principal`.
`JwtIdentityProvider` is one of its providers - it claims `Credential::Bearer`
on every request - and also mints the gateway's HS256 sessions and their
stateless refresh tokens. Register it first so a bearer token is verified
before the password provider is consulted.

`PrincipalMapping` is the reusable half of federated login: discovery, PKCE and
JWKS rotation are the IdP's job, and which claim carries the roles is what every
project otherwise re-derives by reading its provider's token in a debugger.

## Sessions and refresh

The session token is a short HS256 JWT (`JwtIdentityProvider::hmac`); its
lifetime *is* the revocation window, so it defaults to fifteen minutes. The
refresh token is a second JWT (`TokenUse::Refresh`), longer-lived, carrying the
principal and - for the password path - an opaque `epoch` fingerprint of the
stored credential (`auth_epoch`). `refresh` hands what the token carried to a
`resolve` closure and issues tokens for whatever it returns: re-read the user
there so roles and account status are current, and reject when the fingerprint
no longer matches. There is no server-side record: no per-device logout, and no
replay detection beyond that check. Keep `refresh_ttl` short.

`JwtIdentityProvider::jwks` (feature `jwks`) and `::public_key` verify a third
party's bearer token instead - a SPA runs the OIDC code flow itself and presents
the IdP's JWT.

The JWT's `iss` is **this gateway**; which provider authenticated the subject
goes in a separate `idp` claim. Conflating them makes a gateway reject its own
freshly issued token.

## Not in scope

- **No OIDC redirect flow.** A SPA runs the code flow client-side; a
  server-rendered app that wants it pulls `openidconnect` itself.
- **No identity federation.** No code for brokering upstream providers behind
  one endpoint, and no multi-issuer session type. Normal deployments trust one
  issuer: their own HMAC secret, or one IdP's JWKS. `ProviderRegistry` iterates
  whatever you register, so accepting more than one is possible - it is a
  trust-surface choice you make, not something the toolbox builds for you.

Password verification runs even for an unknown username, against a dummy hash,
and off the async runtime. Returning early leaks which usernames exist through
response time.

`scripts/hash-password.sh` prints a PHC hash for seeding a user store.
