# toolbox-auth

Identity, with no transport dependency.

A gRPC backend validating a token should not have to compile axum, which is why
this is its own crate.

| Module | What it holds |
|---|---|
| `principal` | `Principal`, the `Role` trait, `AuthError` |
| `provider` | `IdentityProvider`, `ProviderRegistry`, `Credential`, `UserStore` |
| `provider::password` | `PasswordProvider` over argon2 (feature `password`) |
| `provider::oidc` | `OidcProvider` (feature `oidc`) |
| `provider::proxy_header` | trusting an authenticating reverse proxy |
| `provider::claims_mapping` | where roles live in a provider's token |
| `session` | `JwtCodec`, `SessionCodec`, `Claims` |
| `refresh` | `RefreshTokens`, rotated over the key-value store |

The three providers sit under `provider/` rather than beside it, so the answer
to "what can a deployment log in with?" is one directory listing. Every name is
still re-exported flat, so `toolbox_auth::PasswordProvider` keeps working.

`ClaimsMapping` is the reason OIDC belongs in a toolbox at all: discovery, PKCE
and JWKS rotation are `openidconnect`'s job, and what every project re-derives
by reading its provider's token in a debugger is which claim carries the roles.

The JWT's `iss` is **this gateway**; which provider authenticated the subject
goes in a separate `idp` claim. Conflating them makes a gateway reject its own
freshly issued token.

Password verification runs even for an unknown username, against a dummy hash.
Returning early leaks which usernames exist through response time.

`scripts/hash-password.sh` prints a PHC hash for seeding a user store.
