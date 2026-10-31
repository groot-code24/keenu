// SPDX-License-Identifier: GPL-3.0-or-later

//! OAuth 2.0 for desktop applications: the Authorization Code flow with PKCE (RFC 7636) and a loopback
//! redirect (RFC 8252). Tokens only ever live in the credential store.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keenu_security::credentials::CredentialStore;
use serde::{Deserialize, Serialize};

use crate::http;

/// A service's OAuth endpoints and this application's registration with it.
#[derive(Debug, Clone)]
pub struct Provider {
    pub name: &'static str,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: &'static str,
    pub client_id: String,
    /// Google gives desktop clients a "secret" that is not one; others have none.
    pub client_secret: Option<String>,
    pub extra_auth_params: &'static [(&'static str, &'static str)],
}

/// Tokens as kept in the credential store.
#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// When `access_token` stops working.
    pub expires_at_ms: i64,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tokens { .. }")
    }
}

/// Why signing in or refreshing failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthError {
    /// The user refused, or access was revoked: they have to sign in again.
    Denied(String),
    /// No answer from the service.
    Offline(String),
    /// Anything else, in words for the user.
    Failed(String),
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(why) | Self::Offline(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

/// A sign-in under way: the page to open, and what comes back from it.
pub struct SignIn {
    pub url: String,
    listener: TcpListener,
    redirect_uri: String,
    state: String,
    verifier: String,
}

/// Starts signing in: listens on a free port of this computer, and gives the address to open.
pub fn begin(provider: &Provider) -> Result<SignIn, OAuthError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|err| OAuthError::Failed(format!("could not listen for the sign-in: {err}")))?;
    let port = listener
        .local_addr()
        .map_err(|err| OAuthError::Failed(err.to_string()))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let random = |len| keenu_security::random_hex(len).map_err(OAuthError::Failed);
    let (state, verifier) = (random(16)?, random(32)?);
    let challenge = keenu_security::integrity::pkce_challenge(&verifier);
    let mut pairs = vec![
        ("response_type", "code"),
        ("client_id", provider.client_id.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("scope", provider.scopes),
        ("state", state.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ];
    pairs.extend_from_slice(provider.extra_auth_params);
    let url = format!("{}?{}", provider.auth_url, http::form(&pairs));
    Ok(SignIn {
        url,
        listener,
        redirect_uri,
        state,
        verifier,
    })
}

impl SignIn {
    /// Waits for the browser to come back, then trades its code for tokens.
    pub fn finish(self, provider: &Provider, timeout: Duration) -> Result<Tokens, OAuthError> {
        let code = wait_for_code(&self.listener, &self.state, timeout)?;
        let mut pairs = vec![
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("client_id", provider.client_id.as_str()),
            ("code_verifier", self.verifier.as_str()),
        ];
        if let Some(secret) = &provider.client_secret {
            pairs.push(("client_secret", secret));
        }
        token_request(provider, &pairs, None)
    }
}

/// Accepts connections until one carries the right `state`; anything else is answered and ignored, since
/// any page in a browser can reach a loopback port.
fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    timeout: Duration,
) -> Result<String, OAuthError> {
    listener
        .set_nonblocking(true)
        .map_err(|err| OAuthError::Failed(err.to_string()))?;
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            return Err(OAuthError::Failed("The sign-in timed out.".into()));
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(err) => return Err(OAuthError::Failed(err.to_string())),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buffer = [0u8; 8192];
        let read = stream.read(&mut buffer).unwrap_or(0);
        let request = String::from_utf8_lossy(&buffer[..read]);
        let query = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|path| path.strip_prefix("/callback?"))
            .unwrap_or_default();
        let right_state = http::query_value(query, "state").is_some_and(|sent| {
            keenu_security::constant_time_eq(sent.as_bytes(), state.as_bytes())
        });
        if !right_state {
            respond(
                &mut stream,
                "400 Bad Request",
                "This is not a sign-in Keenu is waiting for.",
            );
            continue;
        }
        if let Some(error) = http::query_value(query, "error") {
            respond(
                &mut stream,
                "200 OK",
                "Sign-in cancelled. You can close this tab.",
            );
            return Err(OAuthError::Denied(format!(
                "The sign-in was refused: {error}."
            )));
        }
        return match http::query_value(query, "code") {
            Some(code) if !code.is_empty() => {
                respond(
                    &mut stream,
                    "200 OK",
                    "Signed in to Keenu. You can close this tab.",
                );
                Ok(code)
            }
            _ => {
                respond(&mut stream, "400 Bad Request", "The sign-in sent no code.");
                Err(OAuthError::Failed("The sign-in sent no code.".into()))
            }
        };
    }
}

fn respond(stream: &mut std::net::TcpStream, status: &str, text: &str) {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Keenu</title><p style=\"font:16px system-ui\">{text}</p>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Gets fresh tokens with `refresh_token`.
pub fn refresh(provider: &Provider, refresh_token: &str) -> Result<Tokens, OAuthError> {
    let mut pairs = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", provider.client_id.as_str()),
    ];
    if let Some(secret) = &provider.client_secret {
        pairs.push(("client_secret", secret));
    }
    token_request(provider, &pairs, Some(refresh_token))
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

fn token_request(
    provider: &Provider,
    pairs: &[(&str, &str)],
    old_refresh: Option<&str>,
) -> Result<Tokens, OAuthError> {
    if !http::is_secure_url(&provider.token_url) {
        return Err(OAuthError::Failed(
            "The token address is not secure.".into(),
        ));
    }
    let body = http::form(pairs);
    let answer = http::send(
        "POST",
        &provider.token_url,
        &[
            ("Content-Type", "application/x-www-form-urlencoded"),
            ("Accept", "application/json"),
        ],
        Some(body.as_bytes()),
    )
    .map_err(|err| OAuthError::Offline(format!("Could not reach {}: {err}", provider.name)))?;
    if !answer.is_success() {
        let reply: serde_json::Value = serde_json::from_slice(&answer.body).unwrap_or_default();
        let code = reply["error"].as_str().unwrap_or_default();
        return Err(if code == "invalid_grant" {
            OAuthError::Denied(format!(
                "{} no longer accepts this sign-in. Sign in again.",
                provider.name
            ))
        } else {
            // The code alone: the description can echo what was sent.
            OAuthError::Failed(format!(
                "{} refused the sign-in ({}{}).",
                provider.name,
                answer.status,
                if code.is_empty() {
                    String::new()
                } else {
                    format!(", {code}")
                }
            ))
        });
    }
    let reply: TokenAnswer = serde_json::from_slice(&answer.body)
        .map_err(|_| OAuthError::Failed(format!("{} sent an unreadable answer.", provider.name)))?;
    Ok(Tokens {
        access_token: reply.access_token,
        // A refresh keeps the old token unless a new one came.
        refresh_token: reply
            .refresh_token
            .or_else(|| old_refresh.map(str::to_owned)),
        expires_at_ms: now_ms() + reply.expires_in.unwrap_or(3600) * 1000,
    })
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// One account's tokens: from the credential store, refreshed when they are about to run out.
pub struct TokenSource {
    provider: Provider,
    store: Arc<dyn CredentialStore>,
    key: String,
}

/// A token this close to running out is refreshed first.
const REFRESH_MARGIN_MS: i64 = 60_000;

impl TokenSource {
    pub fn new(provider: Provider, store: Arc<dyn CredentialStore>, key: String) -> Self {
        Self {
            provider,
            store,
            key,
        }
    }

    /// Saves the tokens of a finished sign-in.
    pub fn save(&self, tokens: &Tokens) -> Result<(), OAuthError> {
        let json =
            serde_json::to_string(tokens).map_err(|err| OAuthError::Failed(err.to_string()))?;
        self.store
            .set(&self.key, &json)
            .map_err(|err| OAuthError::Failed(err.to_string()))
    }

    /// A working access token.
    pub fn access_token(&self) -> Result<String, OAuthError> {
        let saved = self
            .store
            .get(&self.key)
            .map_err(|err| OAuthError::Failed(err.to_string()))?
            .ok_or_else(|| OAuthError::Denied("Not signed in.".into()))?;
        let tokens: Tokens = serde_json::from_str(&saved).map_err(|_| {
            OAuthError::Denied("The saved sign-in is damaged. Sign in again.".into())
        })?;
        if tokens.expires_at_ms - now_ms() > REFRESH_MARGIN_MS {
            return Ok(tokens.access_token);
        }
        self.force_refresh()
    }

    /// Refreshes whatever the expiry says, as after a server has answered 401.
    pub fn force_refresh(&self) -> Result<String, OAuthError> {
        let saved = self
            .store
            .get(&self.key)
            .map_err(|err| OAuthError::Failed(err.to_string()))?
            .ok_or_else(|| OAuthError::Denied("Not signed in.".into()))?;
        let tokens: Tokens = serde_json::from_str(&saved).map_err(|_| {
            OAuthError::Denied("The saved sign-in is damaged. Sign in again.".into())
        })?;
        let Some(refresh_token) = &tokens.refresh_token else {
            return Err(OAuthError::Denied(
                "The sign-in has run out. Sign in again.".into(),
            ));
        };
        let fresh = refresh(&self.provider, refresh_token)?;
        self.save(&fresh)?;
        Ok(fresh.access_token)
    }

    /// Forgets the account's tokens here. A service that offers it should be told first with `revoke`.
    pub fn forget(&self) -> Result<(), OAuthError> {
        self.store
            .delete(&self.key)
            .map_err(|err| OAuthError::Failed(err.to_string()))
    }

    /// The refresh token, for revoking it.
    pub fn refresh_token(&self) -> Option<String> {
        let saved = self.store.get(&self.key).ok()??;
        serde_json::from_str::<Tokens>(&saved).ok()?.refresh_token
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::test_server::serve;
    use keenu_security::credentials::MemoryStore;
    use std::thread;

    fn provider(token_url: &str) -> Provider {
        Provider {
            name: "Test",
            auth_url: "https://auth.invalid/authorize".into(),
            token_url: token_url.into(),
            scopes: "calendar offline",
            client_id: "client".into(),
            client_secret: None,
            extra_auth_params: &[("access_type", "offline")],
        }
    }

    fn get(url: &str) -> u16 {
        http::send("GET", url, &[], None).unwrap().status
    }

    #[test]
    fn the_sign_in_address_carries_pkce_and_state() {
        let sign_in = begin(&provider("https://auth.invalid/token")).unwrap();
        let query = sign_in.url.split_once('?').unwrap().1;
        assert_eq!(
            http::query_value(query, "code_challenge_method").as_deref(),
            Some("S256")
        );
        assert_eq!(
            http::query_value(query, "code_challenge"),
            Some(keenu_security::integrity::pkce_challenge(&sign_in.verifier))
        );
        assert_eq!(
            http::query_value(query, "state"),
            Some(sign_in.state.clone())
        );
        assert_eq!(
            http::query_value(query, "access_type").as_deref(),
            Some("offline")
        );
        assert!(
            http::query_value(query, "redirect_uri")
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
        assert!(
            !sign_in.url.contains(&sign_in.verifier),
            "the verifier stays here"
        );
    }

    #[test]
    fn a_redirect_with_the_wrong_state_is_ignored() {
        let sign_in = begin(&provider("https://auth.invalid/token")).unwrap();
        let (redirect, state) = (sign_in.redirect_uri.clone(), sign_in.state.clone());
        let browser = thread::spawn(move || {
            let attacker = get(&format!("{redirect}?code=evil&state=wrong"));
            let real = get(&format!("{redirect}?code=good&state={state}"));
            (attacker, real)
        });
        let code =
            wait_for_code(&sign_in.listener, &sign_in.state, Duration::from_secs(10)).unwrap();
        assert_eq!(code, "good");
        assert_eq!(browser.join().unwrap(), (400, 200));
    }

    #[test]
    fn a_refused_sign_in_says_so() {
        let sign_in = begin(&provider("https://auth.invalid/token")).unwrap();
        let (redirect, state) = (sign_in.redirect_uri.clone(), sign_in.state.clone());
        let browser =
            thread::spawn(move || get(&format!("{redirect}?error=access_denied&state={state}")));
        let result = wait_for_code(&sign_in.listener, &sign_in.state, Duration::from_secs(10));
        browser.join().unwrap();
        assert!(matches!(result, Err(OAuthError::Denied(_))));
    }

    #[test]
    fn waiting_gives_up_at_the_timeout() {
        let sign_in = begin(&provider("https://auth.invalid/token")).unwrap();
        let result = wait_for_code(&sign_in.listener, &sign_in.state, Duration::from_millis(50));
        assert!(matches!(result, Err(OAuthError::Failed(_))));
    }

    #[test]
    fn refreshing_keeps_the_old_refresh_token_when_no_new_one_comes() {
        let server = serve(vec![(
            200,
            vec![],
            r#"{"access_token":"new","expires_in":3600}"#.into(),
        )]);
        let tokens = refresh(&provider(&format!("{}/token", server.url)), "old-refresh").unwrap();
        assert_eq!(tokens.access_token, "new");
        assert_eq!(tokens.refresh_token.as_deref(), Some("old-refresh"));
        let seen = server.seen.lock().unwrap();
        assert!(seen[0].body.contains("grant_type=refresh_token"));
        assert!(seen[0].body.contains("refresh_token=old-refresh"));
    }

    #[test]
    fn a_revoked_account_has_to_sign_in_again() {
        let server = serve(vec![(400, vec![], r#"{"error":"invalid_grant"}"#.into())]);
        let result = refresh(&provider(&format!("{}/token", server.url)), "revoked");
        assert!(matches!(result, Err(OAuthError::Denied(_))));
    }

    #[test]
    fn a_secret_in_an_error_answer_is_not_repeated() {
        let server = serve(vec![(
            401,
            vec![],
            r#"{"error":"invalid_client","error_description":"bad secret: hunter2"}"#.into(),
        )]);
        let result = refresh(&provider(&format!("{}/token", server.url)), "token");
        let text = result.unwrap_err().to_string();
        assert!(text.contains("invalid_client"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
    }

    #[test]
    fn tokens_never_go_to_an_insecure_address() {
        let result = refresh(&provider("http://auth.example.org/token"), "token");
        assert!(matches!(result, Err(OAuthError::Failed(_))));
    }

    #[test]
    fn a_token_source_uses_a_good_token_and_refreshes_an_old_one() {
        let server = serve(vec![(
            200,
            vec![],
            r#"{"access_token":"fresh","expires_in":3600}"#.into(),
        )]);
        let store: Arc<dyn CredentialStore> = Arc::new(MemoryStore::default());
        let source = TokenSource::new(
            provider(&format!("{}/token", server.url)),
            store,
            "calendar/a".into(),
        );
        assert!(matches!(source.access_token(), Err(OAuthError::Denied(_))));

        let valid = Tokens {
            access_token: "still-good".into(),
            refresh_token: Some("r".into()),
            expires_at_ms: now_ms() + 3_600_000,
        };
        source.save(&valid).unwrap();
        assert_eq!(source.access_token().unwrap(), "still-good");

        source
            .save(&Tokens {
                expires_at_ms: now_ms() + 1000,
                ..valid
            })
            .unwrap();
        assert_eq!(source.access_token().unwrap(), "fresh");
        assert_eq!(
            source.access_token().unwrap(),
            "fresh",
            "saved after refreshing"
        );

        source.forget().unwrap();
        assert!(matches!(source.access_token(), Err(OAuthError::Denied(_))));
    }

    #[test]
    fn tokens_do_not_print_themselves() {
        let tokens = Tokens {
            access_token: "secret".into(),
            refresh_token: None,
            expires_at_ms: 0,
        };
        assert!(!format!("{tokens:?}").contains("secret"));
    }
}
