# toolbox-auth

Identity - principals, roles, providers, JWT sessions - with no transport
dependency.

| Module                         | What it is                                                  |
| ------------------------------ | ----------------------------------------------------------- |
| `principal`                    | the principal, the role trait, and the auth error           |
| `principal::mapping`           | mapping a provider's token claims onto a principal          |
| `provider`                     | the identity-provider trait and the registry that runs them |
| `provider::password`           | password authentication over a user store                   |
| `provider::proxy_header`       | trusting an authenticating reverse proxy                    |
| `provider::asserted_principal` | trusting a principal a gateway already resolved             |
| `provider::jwt`                | minting and verifying JWT sessions and refresh tokens       |
