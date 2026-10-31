// SPDX-License-Identifier: GPL-3.0-or-later

//! One HTTP agent for every request, and the small things every integration needs: refusing insecure addresses,
//! encoding URLs and backing off when told to.
//!
//! Connections are not reused. A server may close one a moment after answering without having said it would (Radicale
//! does), and until then it looks fine, so a request sent on it is lost: a sync fails, or worse, an edit. Nothing here
//! makes enough requests for a new connection each time to matter.

use std::io::Read;
use std::sync::LazyLock;
use std::time::Duration;

use ureq::http::{Method, Request};
use ureq::typestate::WithoutBody;
use ureq::{Agent, RequestBuilder, SendBody};

/// Largest answer read into memory; calendars and manifests are far smaller.
const MAX_BODY: u64 = 64 * 1024 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

static AGENT: LazyLock<Agent> = LazyLock::new(|| {
    let builder = Agent::config_builder()
        // Each caller decides what a 404 or a 412 means.
        .http_status_as_error(false)
        // CalDAV's PROPFIND and REPORT.
        .allow_non_standard_methods(true)
        .max_idle_connections(0)
        .max_idle_connections_per_host(0)
        .user_agent(concat!("Keenu/", env!("CARGO_PKG_VERSION")));
    #[cfg(not(target_os = "linux"))]
    let builder = builder.tls_config(
        ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .build(),
    );
    builder.build().into()
});

/// For callers who treat an error status as an error.
static STRICT_AGENT: LazyLock<Agent> = LazyLock::new(|| {
    let builder = Agent::config_builder()
        .max_idle_connections(0)
        .max_idle_connections_per_host(0);
    #[cfg(not(target_os = "linux"))]
    let builder = builder.tls_config(
        ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .build(),
    );
    builder.build().into()
});

/// A GET of `url` that gives up after `timeout` in all, with statuses of 400 and above as errors.
pub fn get(url: &str, timeout: Duration) -> RequestBuilder<WithoutBody> {
    STRICT_AGENT
        .get(url)
        .config()
        .timeout_global(Some(timeout))
        .build()
}

/// The agent for large downloads: statuses are the caller's to read, with no overall time limit.
pub fn agent_for_downloads() -> &'static Agent {
    &AGENT
}

/// An answer, whatever its status.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// `Retry-After` in seconds; its date form is treated as absent.
    pub fn retry_after(&self) -> Option<u64> {
        self.header("retry-after")?.trim().parse().ok()
    }
}

/// Sends a request and reads the whole answer. `Err` means no answer came: offline, or a TLS failure.
pub fn send(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<Response, String> {
    let method = Method::from_bytes(method.as_bytes()).map_err(|err| err.to_string())?;
    let mut builder = Request::builder().method(method).uri(url);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let answer = match body {
        Some(body) => {
            let request = builder.body(body).map_err(|err| err.to_string())?;
            let request = AGENT
                .configure_request(request)
                .timeout_global(Some(DEFAULT_TIMEOUT))
                .build();
            AGENT.run(request)
        }
        None => {
            let request = builder
                .body(SendBody::none())
                .map_err(|err| err.to_string())?;
            let request = AGENT
                .configure_request(request)
                .timeout_global(Some(DEFAULT_TIMEOUT))
                .build();
            AGENT.run(request)
        }
    };
    let mut answer = answer.map_err(|err| err.to_string())?;
    let status = answer.status().as_u16();
    let headers = answer
        .headers()
        .iter()
        .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_owned())))
        .collect();
    let mut bytes = Vec::new();
    answer
        .body_mut()
        .as_reader()
        .take(MAX_BODY)
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(Response {
        status,
        headers,
        body: bytes,
    })
}

/// Whether credentials or a download may use `url`: HTTPS, or plain HTTP only to this very computer.
pub fn is_secure_url(url: &str) -> bool {
    if url.len() > 8 && url[..8].eq_ignore_ascii_case("https://") {
        return true;
    }
    if !url
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
    {
        return false;
    }
    let host = url[7..].split(['/', '?', '#']).next().unwrap_or_default();
    if host.contains('@') {
        return false;
    }
    let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    matches!(name, "127.0.0.1" | "[::1]" | "localhost")
}

/// `value` as one component of a URL.
pub fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `key=value&...`, encoded.
pub fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", encode_component(key), encode_component(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// One value out of a query string or form, decoded.
pub fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (decode_component(name) == key).then(|| decode_component(value))
    })
}

/// Undoes URL encoding, with `+` as a space, as forms have it.
pub fn decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' if bytes.len() - at >= 3 => {
                let high = (bytes[at + 1] as char).to_digit(16);
                let low = (bytes[at + 2] as char).to_digit(16);
                if let (Some(high), Some(low)) = (high, low) {
                    out.push((high * 16 + low) as u8);
                    at += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            byte => out.push(byte),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// How long to wait before try number `attempt` (from 0): doubling from a second, capped at a minute,
/// and what the server asked for if it asked.
pub fn backoff(attempt: u32, retry_after: Option<u64>) -> Duration {
    let doubled = 1u64 << attempt.min(6);
    Duration::from_secs(retry_after.unwrap_or(doubled).min(60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_this_computer_is_secure() {
        for good in [
            "https://dav.example.org/cal",
            "HTTPS://dav.example.org",
            "http://127.0.0.1:5232/cal",
            "http://localhost:8080",
            "http://[::1]:80/x",
        ] {
            assert!(is_secure_url(good), "{good}");
        }
        for bad in [
            "http://dav.example.org",
            "http://127.0.0.1.evil.example/x",
            "http://localhost@evil.example/",
            "http://localhost.evil.example",
            "ftp://localhost",
            "javascript:alert(1)",
            "",
        ] {
            assert!(!is_secure_url(bad), "{bad}");
        }
    }

    #[test]
    fn encoding_round_trips_what_urls_cannot_hold() {
        let value = "a b&c=d/é?+";
        let encoded = encode_component(value);
        assert_eq!(encoded, "a%20b%26c%3Dd%2F%C3%A9%3F%2B");
        assert_eq!(decode_component(&encoded), value);
        assert_eq!(decode_component("a+b%zz%2"), "a b%zz%2");
    }

    #[test]
    fn reads_values_from_a_query() {
        let query = "code=abc%2Fd&state=x+y&empty=";
        assert_eq!(query_value(query, "code").as_deref(), Some("abc/d"));
        assert_eq!(query_value(query, "state").as_deref(), Some("x y"));
        assert_eq!(query_value(query, "empty").as_deref(), Some(""));
        assert_eq!(query_value(query, "nope"), None);
    }

    #[test]
    fn backs_off_doubling_and_within_reason() {
        assert_eq!(backoff(0, None).as_secs(), 1);
        assert_eq!(backoff(3, None).as_secs(), 8);
        assert_eq!(backoff(30, None).as_secs(), 60);
        assert_eq!(backoff(0, Some(7)).as_secs(), 7);
        assert_eq!(backoff(0, Some(100_000)).as_secs(), 60);
    }

    #[test]
    fn a_server_that_hangs_up_after_each_answer_is_not_mistaken_for_an_outage() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        const REQUESTS: usize = 100;
        // Radicale does this: it answers without saying it will close, then closes a moment later. Until then the
        // connection looks fine, and a request sent on it is lost.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().take(REQUESTS) {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap() > 2 {
                    line.clear();
                }
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK
Content-Length: 2

ok",
                    )
                    .unwrap();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });
        for request in 0..REQUESTS {
            let answer = send("GET", &url, &[], None)
                .unwrap_or_else(|err| panic!("request {request}: {err}"));
            assert_eq!(answer.text(), "ok");
        }
    }
}
