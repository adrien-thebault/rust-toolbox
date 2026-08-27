//! OIDC via `openidconnect`.
//!
//! `openidconnect` does discovery, PKCE, ID token validation and JWKS rotation -
//! **if you find yourself writing JWKS caching or a PKCE verifier, stop**;
//! getting those wrong is a security bug, not a style one. What this adds is
//! the part that is deployment-specific and that every project otherwise
//! re-derives: [`ClaimsMapping`], and the non-negotiables below applied by
//! default rather than by remembering.
//!
//! # Non-negotiable, and not configurable
//!
//! - **PKCE S256 always**, even with a client secret.
//! - `state` and `nonce` generated and checked.
//! - `aud` checked against the client id.
//! - Clock skew leeway on the ID token.
//!
//! # Mode
//!
//! This defaults to **BFF / session exchange**: the gateway runs the code
//! flow and mints its own session, the browser never sees the provider's
//! token, and everything downstream is identical whether the login was a
//! password or Keycloak.

use std::collections::BTreeMap;

use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    reqwest,
};
use tracing::warn;

use super::claims_mapping::ClaimsMapping;
use crate::principal::{AuthError, Principal};

/// What must be remembered between the redirect out and the callback back.
///
/// The verifier and the nonce are secrets for the duration of one login: store
/// them server-side against the state, never in a cookie the browser can read.
#[derive(Debug)]
pub struct AuthSession {
    /// The CSRF state echoed by the provider.
    pub state: String,
    /// The PKCE verifier, which proves this callback belongs to this request.
    pub pkce_verifier: String,
    /// The nonce, which binds the ID token to this request.
    pub nonce: String,
    /// Where to send the browser afterwards.
    pub redirect_to: Option<String>,
}

/// An OIDC identity provider.
pub struct OidcProvider {
    id: String,
    display_name: String,
    client: CoreClient<
        openidconnect::EndpointSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointMaybeSet,
        openidconnect::EndpointMaybeSet,
    >,
    http: reqwest::Client,
    mapping: ClaimsMapping,
    scopes: Vec<String>,
}

impl std::fmt::Debug for OidcProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcProvider")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl OidcProvider {
    /// Discover a provider and build a client for it.
    ///
    /// Discovery is a network call, so this is done once at startup rather
    /// than per login.
    ///
    /// # Arguments
    ///
    /// * `id` - This provider's name in the registry, and the value that
    ///   becomes `Principal::issuer`. Not the OIDC client id.
    /// * `issuer_url` - The issuer's base URL. Discovery appends the well-known
    ///   path itself.
    /// * `client_id` - The OIDC client id registered with the provider.
    /// * `client_secret` - The client secret, or `None` for a public client.
    ///   PKCE is used either way.
    /// * `redirect_url` - Where the provider sends the browser back. It must
    ///   match what is registered, exactly.
    /// * `mapping` - How to read roles and attributes out of this provider's
    ///   claims.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when discovery fails or a URL does not parse.
    pub async fn discover(
        id: impl Into<String>,
        issuer_url: &str,
        client_id: &str,
        client_secret: Option<&str>,
        redirect_url: &str,
        mapping: ClaimsMapping,
    ) -> Result<Self, AuthError> {
        let http = reqwest::ClientBuilder::new()
            // An identity provider that redirects is either misconfigured or
            // being impersonated; neither is worth following.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| AuthError::Malformed(e.to_string()))?;

        let issuer = IssuerUrl::new(issuer_url.to_owned())
            .map_err(|e| AuthError::Malformed(format!("issuer url: {e}")))?;
        let metadata = CoreProviderMetadata::discover_async(issuer, &http)
            .await
            .map_err(|e| AuthError::Malformed(format!("oidc discovery: {e}")))?;

        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(client_id.to_owned()),
            client_secret.map(|s| ClientSecret::new(s.to_owned())),
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_url.to_owned())
                .map_err(|e| AuthError::Malformed(format!("redirect url: {e}")))?,
        );

        Ok(Self {
            id: id.into(),
            display_name: "Single sign-on".to_owned(),
            client,
            http,
            mapping,
            scopes: vec!["email".to_owned(), "profile".to_owned()],
        })
    }

    /// Override the button label.
    ///
    /// # Arguments
    ///
    /// * `name` - What a login page shows on the button, when the id is not
    ///   what a user should read.
    #[must_use]
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }

    /// Request additional scopes.
    ///
    /// # Arguments
    ///
    /// * `scopes` - Scopes to request on top of `openid`, `profile` and
    ///   `email`, for a provider that gates roles behind one.
    #[must_use]
    pub fn scopes<I: IntoIterator<Item = S>, S: Into<String>>(mut self, scopes: I) -> Self {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }

    /// This provider's id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The button label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.display_name
    }

    /// Where to send the browser, and what to remember while it is gone.
    ///
    /// # Arguments
    ///
    /// * `redirect_to` - Where to send the user after login. It is kept
    ///   server-side in the returned session rather than round-tripped through
    ///   the browser, so it cannot be tampered with.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when the authorization endpoint is missing.
    pub fn authorize_url(
        &self,
        redirect_to: Option<&str>,
    ) -> Result<(String, AuthSession), AuthError> {
        // S256 always, even with a client secret: the secret protects the
        // token exchange, PKCE protects the authorization code, and they are
        // different attacks.
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();

        let mut request = self
            .client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .set_pkce_challenge(challenge);

        for scope in &self.scopes {
            request = request.add_scope(Scope::new(scope.clone()));
        }

        let (url, state, nonce) = request.url();
        Ok((
            url.to_string(),
            AuthSession {
                state: state.secret().clone(),
                pkce_verifier: verifier.secret().clone(),
                nonce: nonce.secret().clone(),
                redirect_to: redirect_to.map(ToOwned::to_owned),
            },
        ))
    }

    /// Exchange an authorization code for a principal.
    ///
    /// # Arguments
    ///
    /// * `code` - The authorization code the provider sent back.
    /// * `returned_state` - The state as it came back. It is compared with the
    ///   stored one before anything else happens.
    /// * `session` - What was remembered at [`OidcProvider::authorize_url`]:
    ///   the state, the PKCE verifier and the nonce.
    ///
    /// # Errors
    /// [`AuthError::Unauthenticated`] when the state does not match, the
    /// exchange fails, or the ID token does not validate.
    pub async fn exchange(
        &self,
        code: &str,
        returned_state: &str,
        session: &AuthSession,
    ) -> Result<Principal, AuthError> {
        // Constant-time is not needed here - the state is not a secret an
        // attacker guesses byte by byte - but a mismatch is a CSRF attempt and
        // must not proceed.
        if returned_state != session.state {
            warn!("an OIDC callback arrived with a state that does not match");
            return Err(AuthError::Unauthenticated);
        }

        let tokens = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .map_err(|e| AuthError::Malformed(e.to_string()))?
            .set_pkce_verifier(PkceCodeVerifier::new(session.pkce_verifier.clone()))
            .request_async(&self.http)
            .await
            .map_err(|e| {
                warn!(error = %e, "the OIDC token exchange failed");
                AuthError::Unauthenticated
            })?;

        let id_token = tokens
            .id_token()
            .ok_or_else(|| AuthError::Malformed("the provider returned no ID token".to_owned()))?;

        // Validates the signature against the discovered JWKS, the audience
        // against the client id, the issuer, the expiry with leeway, and the
        // nonce against this request. All of it is openidconnect's, and none of
        // it should be reimplemented.
        let verifier = self.client.id_token_verifier();
        let claims = id_token
            .claims(&verifier, &Nonce::new(session.nonce.clone()))
            .map_err(|e| {
                warn!(error = %e, "an OIDC ID token did not validate");
                AuthError::Unauthenticated
            })?;

        let value =
            serde_json::to_value(claims).map_err(|e| AuthError::Malformed(e.to_string()))?;

        self.mapping.apply(&value, &self.id).ok_or_else(|| {
            AuthError::Malformed(format!(
                "the ID token has no `{}` claim, so there is no stable subject",
                self.mapping.subject.as_str()
            ))
        })
    }

    /// The mapping this provider applies, for a test or a diagnostic.
    #[must_use]
    pub fn mapping(&self) -> &ClaimsMapping {
        &self.mapping
    }

    /// The scopes requested, for a diagnostic.
    #[must_use]
    pub fn requested_scopes(&self) -> &[String] {
        &self.scopes
    }

    /// Extra attributes, for a caller that wants everything the token carried.
    ///
    /// # Arguments
    ///
    /// * `claims` - The claims document to copy from, for a caller that wants
    ///   more than the mapping selected.
    #[must_use]
    pub fn extra_attributes(claims: &serde_json::Value) -> BTreeMap<String, String> {
        claims
            .as_object()
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect()
            })
            .unwrap_or_default()
    }
}
