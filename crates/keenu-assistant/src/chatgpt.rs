// SPDX-License-Identifier: GPL-3.0-or-later

//! Sign in with ChatGPT, as OpenAI documents it for open-source and local apps: Keenu registers itself on first sign-in
//! (no client secret, no API key), and the user's plan pays for requests through the public Responses API. Its tokens
//! live in the credential store. Nothing here reaches into any other program's sign-in.
//!
//! <https://developers.openai.com/siwc/token-sharing-open-source>

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use keenu_integrations::http::{form, query_value};
use keenu_security::credentials::CredentialStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where it signs in. Tests point these elsewhere.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub discovery: String,
    pub api: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            authorize: "https://auth.openai.com/api/accounts/authorize".into(),
            token: "https://auth.openai.com/api/accounts/oauth/token".into(),
            discovery: "https://auth.openai.com/.well-known/openid-configuration".into(),
            api: API.into(),
        }
    }
}

/// The API a plan's usage goes through.
pub const API: &str = "https://api.openai.com/v1";
/// The first sign-in registers Keenu, and OpenAI gives back an id of its own to keep.
const FIRST_CLIENT: &str = "dynamic_agent_client";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The permission to use the plan: a valid sign-in alone does not grant it.
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
/// The port the first sign-in must use; later ones may use another.
const FIRST_PORT: u16 = 1455;
const CALLBACK: &str = "/auth/callback";
const CREDENTIAL: &str = "assistant/chatgpt";
/// A token this close to its end is renewed first.
const RENEW_WITHIN_MS: i64 = 60_000;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// What is kept in the credential store.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub client_id: String,
    pub host_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: i64,
    /// The user may have said no to using their plan, though they signed in.
    pub plan_allowed: bool,
    pub model: String,
}

/// A sign-in under way.
pub struct SignIn {
    pub url: String,
    listener: TcpListener,
    redirect_uri: String,
    state: String,
    nonce: String,
    verifier: String,
    client_id: String,
    host_id: String,
    endpoints: Endpoints,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn random(bytes: usize) -> Result<String, String> {
    keenu_security::random_hex(bytes)
}

/// A host's own id: random, kept, and telling nothing about who uses it.
fn new_host_id() -> Result<String, String> {
    let hex = random(16)?;
    Ok(format!(
        "urn:uuid:{}-{}-4{}-a{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    ))
}

/// The session in the store, if any.
pub fn load(store: &dyn CredentialStore) -> Option<Session> {
    let text = store.get(CREDENTIAL).ok().flatten()?;
    serde_json::from_str(&text).ok()
}

fn save(store: &dyn CredentialStore, session: &Session) -> Result<(), String> {
    let text = serde_json::to_string(session).map_err(|err| err.to_string())?;
    store.set(CREDENTIAL, &text).map_err(|err| err.to_string())
}

/// Starts signing in: listens on this computer, and gives the page to open. A session kept from before keeps its ids.
pub fn begin(store: &dyn CredentialStore, endpoints: Endpoints) -> Result<SignIn, String> {
    let known = load(store).unwrap_or_default();
    let port = if known.client_id.is_empty() {
        FIRST_PORT
    } else {
        0
    };
    let listener = TcpListener::bind(("127.0.0.1", port))
        .or_else(|err| {
            // Only a later sign-in may move to another port.
            if known.client_id.is_empty() {
                Err(err)
            } else {
                TcpListener::bind(("127.0.0.1", 0))
            }
        })
        .map_err(|err| {
            format!("Could not listen for the sign-in on port {FIRST_PORT}: {err}. Close whatever uses it and try again.")
        })?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK}");
    let (state, nonce, verifier) = (random(16)?, random(16)?, random(32)?);
    let challenge = keenu_security::integrity::pkce_challenge(&verifier);
    let host_id = if known.host_id.is_empty() {
        new_host_id()?
    } else {
        known.host_id
    };
    let client_id = if known.client_id.is_empty() {
        FIRST_CLIENT.to_owned()
    } else {
        known.client_id
    };
    let mut pairs = vec![
        ("client_id", client_id.as_str()),
        ("ext_agent_host_id", host_id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri.as_str()),
        ("scope", SCOPES),
        ("resource", API),
        ("state", state.as_str()),
        ("nonce", nonce.as_str()),
        ("code_challenge_method", "S256"),
        ("code_challenge", challenge.as_str()),
    ];
    if client_id == FIRST_CLIENT {
        pairs.push(("agent_name_hint", "Keenu"));
    }
    let url = format!("{}?{}", endpoints.authorize, form(&pairs));
    Ok(SignIn {
        url,
        listener,
        redirect_uri,
        state,
        nonce,
        verifier,
        client_id,
        host_id,
        endpoints,
    })
}

/// What the browser came back with.
struct Callback {
    code: String,
    client_id: String,
}

fn answer_browser(stream: &mut TcpStream, status: &str, text: &str) {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Keenu</title><p style=\"font:16px system-ui\">{text}</p>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Waits for the right callback. Any page in a browser can reach this port, so others are answered and ignored.
fn wait_for_callback(sign_in: &SignIn, timeout: Duration) -> Result<Callback, String> {
    sign_in
        .listener
        .set_nonblocking(true)
        .map_err(|err| err.to_string())?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let mut stream = match sign_in.listener.accept() {
            Ok((stream, _)) => stream,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(err) => return Err(err.to_string()),
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
            .and_then(|path| path.strip_prefix(CALLBACK))
            .and_then(|rest| rest.strip_prefix('?'))
            .unwrap_or_default();
        let right = query_value(query, "state").is_some_and(|sent| {
            keenu_security::constant_time_eq(sent.as_bytes(), sign_in.state.as_bytes())
        });
        if !right {
            answer_browser(
                &mut stream,
                "400 Bad Request",
                "This is not a sign-in Keenu is waiting for.",
            );
            continue;
        }
        if let Some(error) = query_value(query, "error") {
            answer_browser(
                &mut stream,
                "200 OK",
                "Nothing was connected. You can close this tab.",
            );
            return Err(if error == "access_denied" {
                "You chose not to let Keenu use your ChatGPT plan. Nothing was connected.".into()
            } else {
                format!("The sign-in was refused: {error}.")
            });
        }
        let Some(code) = query_value(query, "code").filter(|code| !code.is_empty()) else {
            answer_browser(&mut stream, "400 Bad Request", "The sign-in sent no code.");
            return Err("The sign-in sent no code.".into());
        };
        answer_browser(
            &mut stream,
            "200 OK",
            "Connected to Keenu. You can close this tab.",
        );
        return Ok(Callback {
            code,
            client_id: query_value(query, "client_id").unwrap_or_else(|| sign_in.client_id.clone()),
        });
    }
    Err("The sign-in timed out.".into())
}

/// Posts a form to the token address; the JSON answer.
fn post_token(endpoints: &Endpoints, pairs: &[(&str, &str)]) -> Result<Value, TokenError> {
    let mut reply = crate::api::agent(HTTP_TIMEOUT)
        .post(&endpoints.token)
        .content_type("application/x-www-form-urlencoded")
        .send(form(pairs))
        .map_err(|err| TokenError::Temporary(format!("Could not reach ChatGPT: {err}")))?;
    let status = reply.status().as_u16();
    let body = reply.body_mut().read_to_string().unwrap_or_default();
    let parsed: Value = serde_json::from_str(&body).unwrap_or_default();
    if status >= 400 {
        let code = parsed["error"]["code"]
            .as_str()
            .or(parsed["error"].as_str())
            .unwrap_or_default();
        return Err(match code {
            "invalid_grant"
            | "invalid_refresh_token"
            | "token_expired"
            | "refresh_token_expired"
            | "refresh_token_invalidated"
            | "refresh_token_reused" => TokenError::SignInAgain,
            _ if status >= 500 => TokenError::Temporary(format!("ChatGPT answered {status}.")),
            _ => TokenError::Refused(format!("ChatGPT refused the sign-in ({status}: {code}).")),
        });
    }
    Ok(parsed)
}

#[derive(Debug, PartialEq, Eq)]
enum TokenError {
    SignInAgain,
    Temporary(String),
    Refused(String),
}

impl TokenError {
    fn words(self) -> String {
        match self {
            Self::SignInAgain => {
                "ChatGPT's sign-in has ended: connect it again in Settings.".into()
            }
            Self::Temporary(why) | Self::Refused(why) => why,
        }
    }
}

/// The `nonce` an ID token carries, read for comparing with the one sent. The token came straight from OpenAI over HTTPS.
fn nonce_of(id_token: &str) -> Option<String> {
    use base64::Engine;
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims["nonce"].as_str().map(str::to_owned)
}

impl SignIn {
    /// Waits for the browser, trades its code for tokens, and keeps them. What the plan allows is checked, not assumed.
    pub fn finish(self, store: &dyn CredentialStore, timeout: Duration) -> Result<Session, String> {
        let callback = wait_for_callback(&self, timeout)?;
        let answer = post_token(
            &self.endpoints,
            &[
                ("grant_type", "authorization_code"),
                ("client_id", callback.client_id.as_str()),
                ("code", callback.code.as_str()),
                ("code_verifier", self.verifier.as_str()),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("resource", API),
            ],
        )
        .map_err(TokenError::words)?;
        if let Some(id_token) = answer["id_token"].as_str()
            && nonce_of(id_token).as_deref() != Some(self.nonce.as_str())
        {
            return Err(
                "The sign-in's answer did not match what Keenu asked: nothing was connected."
                    .into(),
            );
        }
        let session = Session {
            client_id: callback.client_id,
            host_id: self.host_id,
            access_token: answer["access_token"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            refresh_token: answer["refresh_token"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            expires_at_ms: now_ms() + answer["expires_in"].as_i64().unwrap_or(3600) * 1000,
            plan_allowed: answer["scope"]
                .as_str()
                .is_some_and(|scope| scope.split_whitespace().any(|one| one == PLAN_SCOPE)),
            model: String::new(),
        };
        if session.access_token.is_empty() {
            return Err("ChatGPT sent no token.".into());
        }
        save(store, &session)?;
        if !session.plan_allowed {
            return Err("You are signed in, but ChatGPT did not allow using your plan. Check your plan or try again.".into());
        }
        Ok(session)
    }
}

/// A token that is good now, renewed if it needs to be. Each renewal replaces the refresh token too.
pub fn access_token(store: &dyn CredentialStore, endpoints: &Endpoints) -> Result<String, String> {
    let mut session = load(store).ok_or("ChatGPT is not connected.")?;
    if !session.plan_allowed {
        return Err(
            "Your ChatGPT plan is not available to Keenu: connect it again in Settings.".into(),
        );
    }
    if session.expires_at_ms - now_ms() > RENEW_WITHIN_MS {
        return Ok(session.access_token);
    }
    let answer = post_token(
        endpoints,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", session.client_id.as_str()),
            ("refresh_token", session.refresh_token.as_str()),
            ("resource", API),
        ],
    );
    let answer = match answer {
        Ok(answer) => answer,
        Err(TokenError::SignInAgain) => {
            // Its ids stay, so signing in again is the same registration.
            session.access_token.clear();
            session.refresh_token.clear();
            session.plan_allowed = false;
            save(store, &session)?;
            return Err(TokenError::SignInAgain.words());
        }
        Err(other) => return Err(other.words()),
    };
    session.access_token = answer["access_token"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if let Some(next) = answer["refresh_token"].as_str() {
        session.refresh_token = next.to_owned();
    }
    session.expires_at_ms = now_ms() + answer["expires_in"].as_i64().unwrap_or(3600) * 1000;
    save(store, &session)?;
    Ok(session.access_token)
}

/// The models this account may use, as `/models` lists them.
pub fn models(endpoints: &Endpoints, token: &str) -> Result<Vec<String>, String> {
    let mut reply = crate::api::agent(HTTP_TIMEOUT)
        .get(format!("{}/models", endpoints.api))
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|err| format!("Could not reach ChatGPT: {err}"))?;
    if reply.status().as_u16() >= 400 {
        return Err(format!(
            "ChatGPT would not list its models ({}).",
            reply.status()
        ));
    }
    let body = reply
        .body_mut()
        .read_to_string()
        .map_err(|err| err.to_string())?;
    let parsed: Value = serde_json::from_str(&body).map_err(|err| err.to_string())?;
    let list = parsed["models"].as_array().or(parsed["data"].as_array());
    Ok(list
        .into_iter()
        .flatten()
        .filter(|model| model["visibility"].as_str().is_none_or(|v| v == "list"))
        .filter_map(|model| {
            model["slug"]
                .as_str()
                .or(model["id"].as_str())
                .map(str::to_owned)
        })
        .collect())
}

/// The way to the Responses API with this session, or why not.
pub fn route(
    store: Arc<dyn CredentialStore>,
    endpoints: Endpoints,
    wanted_model: &str,
) -> Result<crate::api::ApiRoute, String> {
    let token = access_token(&*store, &endpoints)?;
    let mut session = load(&*store).ok_or("ChatGPT is not connected.")?;
    let model = if wanted_model.is_empty() {
        if session.model.is_empty() {
            session.model = models(&endpoints, &token)?
                .into_iter()
                .next()
                .ok_or("ChatGPT lists no model for this account.")?;
            save(&*store, &session)?;
        }
        session.model
    } else {
        wanted_model.to_owned()
    };
    let base = endpoints.api.clone();
    Ok(crate::api::ApiRoute {
        wire: crate::api::Wire::Responses,
        base,
        model,
        token: Arc::new(move || access_token(&*store, &endpoints)),
    })
}

/// Signs out: asks OpenAI to end the sign-in, then forgets it here whatever came of that.
pub fn sign_out(store: &dyn CredentialStore, endpoints: &Endpoints) -> Result<(), String> {
    let Some(session) = load(store) else {
        return Ok(());
    };
    let revocation = crate::api::agent(HTTP_TIMEOUT)
        .get(&endpoints.discovery)
        .call()
        .ok()
        .and_then(|mut reply| reply.body_mut().read_to_string().ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|found| found["revocation_endpoint"].as_str().map(str::to_owned));
    let ended = revocation.is_some_and(|address| {
        crate::api::agent(HTTP_TIMEOUT)
            .post(&address)
            .content_type("application/x-www-form-urlencoded")
            .send(form(&[
                ("token", session.refresh_token.as_str()),
                ("token_type_hint", "refresh_token"),
                ("client_id", session.client_id.as_str()),
            ]))
            .is_ok_and(|reply| reply.status().is_success())
    });
    store.delete(CREDENTIAL).map_err(|err| err.to_string())?;
    if ended {
        Ok(())
    } else {
        Err("Forgotten here, but ChatGPT could not be told. To end it there too: chatgpt.com, Settings, Connected apps.".into())
    }
}

#[cfg(test)]
mod tests {
    use keenu_security::credentials::MemoryStore;

    use super::*;
    use crate::api::tests::serve;

    fn endpoints(base: &str) -> Endpoints {
        Endpoints {
            authorize: "https://auth.invalid/authorize".into(),
            token: format!("{base}/token"),
            discovery: format!("{base}/discovery"),
            api: base.to_owned(),
        }
    }

    fn id_token(nonce: &str) -> String {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(format!(r#"{{"nonce":"{nonce}"}}"#));
        format!("x.{payload}.y")
    }

    /// Plays the browser: comes back to the sign-in's callback with `extra`, as the server would send it.
    fn come_back(sign_in: &SignIn, extra: &str) {
        let port = sign_in.listener.local_addr().unwrap().port();
        let state = query_value(sign_in.url.split('?').nth(1).unwrap(), "state").unwrap();
        let extra = extra.to_owned();
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                stream,
                "GET {CALLBACK}?state={state}&{extra} HTTP/1.1\r\n\r\n"
            )
            .unwrap();
            let mut answer = String::new();
            let _ = stream.read_to_string(&mut answer);
        });
    }

    fn session(expires_in_ms: i64) -> Session {
        Session {
            client_id: "oaiapp_1".into(),
            host_id: "urn:uuid:x".into(),
            access_token: "old-access".into(),
            refresh_token: "old-refresh".into(),
            expires_at_ms: now_ms() + expires_in_ms,
            plan_allowed: true,
            model: "gpt-x".into(),
        }
    }

    #[test]
    fn a_first_sign_in_registers_and_asks_for_the_plan() {
        let store = MemoryStore::default();
        // The port may be taken by another test or program; the request's shape is what is checked here.
        let Ok(sign_in) = begin(&store, endpoints("http://127.0.0.1:1")) else {
            return;
        };
        let query = sign_in.url.split('?').nth(1).unwrap().to_owned();
        assert_eq!(
            query_value(&query, "client_id").unwrap(),
            "dynamic_agent_client"
        );
        assert_eq!(query_value(&query, "agent_name_hint").unwrap(), "Keenu");
        assert!(
            query_value(&query, "ext_agent_host_id")
                .unwrap()
                .starts_with("urn:uuid:")
        );
        assert!(query_value(&query, "scope").unwrap().contains(PLAN_SCOPE));
        assert_eq!(query_value(&query, "resource").unwrap(), API);
        assert_eq!(
            query_value(&query, "code_challenge_method").unwrap(),
            "S256"
        );
        assert!(
            sign_in
                .redirect_uri
                .starts_with("http://127.0.0.1:1455/auth/callback")
        );
    }

    #[test]
    fn a_later_sign_in_keeps_its_ids_and_any_port() {
        let store = MemoryStore::default();
        save(&store, &session(0)).unwrap();
        let sign_in = begin(&store, endpoints("http://127.0.0.1:1")).unwrap();
        let query = sign_in.url.split('?').nth(1).unwrap();
        assert_eq!(query_value(query, "client_id").unwrap(), "oaiapp_1");
        assert_eq!(
            query_value(query, "ext_agent_host_id").unwrap(),
            "urn:uuid:x"
        );
        assert!(query_value(query, "agent_name_hint").is_none());
    }

    #[test]
    fn signing_in_keeps_the_issued_id_and_checks_the_permission() {
        let store = MemoryStore::default();
        save(&store, &session(0)).unwrap();
        let answer = |nonce: &str, scope: &str| {
            format!(
                r#"{{"access_token":"a","refresh_token":"r","expires_in":3600,"scope":"{scope}","id_token":"{}"}}"#,
                id_token(nonce)
            )
        };
        // The right nonce and the permission.
        let (base, asked) = serve(vec![(200, String::new())]);
        drop((base, asked));
        let sign_in = begin(&store, endpoints("http://127.0.0.1:1")).unwrap();
        let nonce = sign_in.nonce.clone();
        let (base, asked) = serve(vec![(
            200,
            answer(&nonce, "openid chatgpt.tokens.use.direct"),
        )]);
        let mut sign_in = sign_in;
        sign_in.endpoints = endpoints(&base);
        come_back(&sign_in, "code=abc&client_id=oaiapp_2");
        let done = sign_in.finish(&store, Duration::from_secs(5)).unwrap();
        assert_eq!(done.client_id, "oaiapp_2");
        assert!(done.plan_allowed);
        assert_eq!(load(&store).unwrap().access_token, "a");
        let asked = asked.lock().unwrap();
        assert!(asked[0].contains("code_verifier=") && asked[0].contains("code=abc"));
        // No client secret exists to send.
        assert!(!asked[0].contains("client_secret"));
        drop(asked);

        // Signed in, but the plan's permission was not granted: kept, and said plainly.
        let sign_in = begin(&store, endpoints("http://127.0.0.1:1")).unwrap();
        let nonce = sign_in.nonce.clone();
        let (base, _) = serve(vec![(200, answer(&nonce, "openid"))]);
        let mut sign_in = sign_in;
        sign_in.endpoints = endpoints(&base);
        come_back(&sign_in, "code=abc");
        assert!(
            sign_in
                .finish(&store, Duration::from_secs(5))
                .unwrap_err()
                .contains("did not allow")
        );
        assert!(!load(&store).unwrap().plan_allowed);
        assert!(access_token(&store, &endpoints(&base)).is_err());
    }

    #[test]
    fn an_answer_for_another_sign_in_is_refused() {
        let store = MemoryStore::default();
        save(&store, &session(0)).unwrap();
        let (base, _) = serve(vec![(
            200,
            format!(
                r#"{{"access_token":"a","id_token":"{}"}}"#,
                id_token("other")
            ),
        )]);
        let mut sign_in = begin(&store, endpoints("http://127.0.0.1:1")).unwrap();
        sign_in.endpoints = endpoints(&base);
        come_back(&sign_in, "code=abc");
        assert!(
            sign_in
                .finish(&store, Duration::from_secs(5))
                .unwrap_err()
                .contains("did not match")
        );
        assert_eq!(load(&store).unwrap().access_token, "old-access");
    }

    #[test]
    fn declining_connects_nothing() {
        let store = MemoryStore::default();
        save(&store, &session(0)).unwrap();
        let sign_in = begin(&store, endpoints("http://127.0.0.1:1")).unwrap();
        come_back(&sign_in, "error=access_denied");
        assert!(
            sign_in
                .finish(&store, Duration::from_secs(5))
                .unwrap_err()
                .contains("chose not")
        );
    }

    #[test]
    fn a_callback_with_the_wrong_state_is_ignored() {
        let store = MemoryStore::default();
        save(&store, &session(0)).unwrap();
        let sign_in = begin(&store, endpoints("http://127.0.0.1:1")).unwrap();
        let port = sign_in.listener.local_addr().unwrap().port();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            stream,
            "GET {CALLBACK}?state=wrong&code=evil HTTP/1.1\r\n\r\n"
        )
        .unwrap();
        assert!(
            sign_in
                .finish(&store, Duration::from_millis(400))
                .unwrap_err()
                .contains("timed out")
        );
    }

    #[test]
    fn a_good_token_is_used_and_an_old_one_renewed_with_its_replacement_kept() {
        let store = MemoryStore::default();
        save(&store, &session(3_600_000)).unwrap();
        assert_eq!(
            access_token(&store, &endpoints("http://127.0.0.1:1")).unwrap(),
            "old-access"
        );
        save(&store, &session(0)).unwrap();
        let (base, asked) = serve(vec![(
            200,
            r#"{"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600}"#
                .to_owned(),
        )]);
        assert_eq!(
            access_token(&store, &endpoints(&base)).unwrap(),
            "new-access"
        );
        assert_eq!(load(&store).unwrap().refresh_token, "new-refresh");
        let asked = asked.lock().unwrap();
        assert!(asked[0].contains("grant_type=refresh_token") && asked[0].contains("old-refresh"));
        assert!(!asked[0].contains("scope="));
    }

    #[test]
    fn a_dead_sign_in_asks_for_a_new_one_but_keeps_the_registration() {
        let store = MemoryStore::default();
        save(&store, &session(0)).unwrap();
        let (base, _) = serve(vec![(
            400,
            r#"{"error":"refresh_token_expired"}"#.to_owned(),
        )]);
        assert!(
            access_token(&store, &endpoints(&base))
                .unwrap_err()
                .contains("connect it again")
        );
        let kept = load(&store).unwrap();
        assert_eq!(kept.client_id, "oaiapp_1");
        assert!(kept.refresh_token.is_empty());
    }

    #[test]
    fn a_passing_failure_keeps_the_sign_in() {
        let store = MemoryStore::default();
        save(&store, &session(0)).unwrap();
        let (base, _) = serve(vec![(503, "{}".to_owned())]);
        assert!(access_token(&store, &endpoints(&base)).is_err());
        assert_eq!(load(&store).unwrap().refresh_token, "old-refresh");
    }

    #[test]
    fn lists_only_the_models_meant_for_listing() {
        let (base, _) = serve(vec![(
            200,
            r#"{"models":[{"slug":"a","visibility":"list"},{"slug":"b","visibility":"hidden"}]}"#
                .to_owned(),
        )]);
        assert_eq!(models(&endpoints(&base), "t").unwrap(), ["a"]);
    }

    #[test]
    fn signing_out_tells_openai_and_forgets_it_here() {
        let store = MemoryStore::default();
        save(&store, &session(0)).unwrap();
        // Told nothing where to revoke: still forgotten here, and it says what is left to do.
        let (base, _) = serve(vec![(200, "{}".to_owned())]);
        assert!(
            sign_out(&store, &endpoints(&base))
                .unwrap_err()
                .contains("Forgotten here")
        );
        assert!(load(&store).is_none());
        assert!(sign_out(&store, &endpoints(&base)).is_ok());
    }

    #[test]
    fn a_host_id_says_nothing_about_who_uses_it() {
        let id = new_host_id().unwrap();
        assert!(id.starts_with("urn:uuid:") && id.len() == 45, "{id}");
        assert_ne!(id, new_host_id().unwrap());
    }
}
