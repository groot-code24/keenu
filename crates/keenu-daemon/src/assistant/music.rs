// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Checks the Spotify links the AI answers `play_music` with, so a made-up one never reaches Spotify:
//! a link Spotify doesn't know is dropped, and the extension searches for the music instead.

use std::time::Duration;

use log::warn;
use serde_json::Value;

/// How long Spotify gets to confirm a link.
const CHECK_TIMEOUT: Duration = Duration::from_secs(4);

/// `spotify:track:ID` from a Spotify link or URI (track, album, playlist or artist).
fn spotify_uri(link: &str) -> Option<String> {
    let rest = link
        .strip_prefix("spotify:")
        .map(|rest| rest.replacen(':', "/", 1))
        .or_else(|| {
            let path = link.split_once("open.spotify.com/")?.1;
            // Links may carry a locale ("intl-id/track/…") and a query ("?si=…").
            let path = path.split(['?', '#']).next()?;
            Some(match path.split_once('/') {
                Some((locale, rest)) if locale.starts_with("intl-") => rest.to_owned(),
                _ => path.to_owned(),
            })
        })?;
    let (kind, id) = rest.split_once('/')?;
    let known = ["track", "album", "playlist", "artist"].contains(&kind);
    let valid = id.len() == 22 && id.chars().all(|c| c.is_ascii_alphanumeric());
    (known && valid).then(|| format!("spotify:{kind}:{id}"))
}

/// Whether Spotify knows `uri`, asked through its public oEmbed page. Blocking.
fn exists(uri: &str) -> bool {
    let link = format!(
        "https://open.spotify.com/{}",
        uri["spotify:".len()..].replacen(':', "/", 1)
    );
    match crate::http::get("https://open.spotify.com/oembed", CHECK_TIMEOUT)
        .query("url", &link)
        .call()
    {
        Ok(_) => true,
        Err(err) => {
            warn!("Spotify does not know {uri}: {err}");
            false
        }
    }
}

/// Keeps a `play_music` action's `uri` only if it is a Spotify link Spotify knows, as `spotify:…`.
pub async fn check(actions: &mut [Value]) {
    for action in actions
        .iter_mut()
        .filter(|action| action["type"] == "play_music")
    {
        let uri = action["uri"].as_str().and_then(spotify_uri);
        let known = match uri.clone() {
            Some(uri) => tokio::task::spawn_blocking(move || exists(&uri))
                .await
                .unwrap_or(false),
            None => false,
        };
        match (known, action.as_object_mut()) {
            (true, Some(fields)) => {
                fields.insert("uri".into(), Value::from(uri));
            }
            (false, Some(fields)) => {
                fields.remove("uri");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_spotify_links() {
        let id = "4uLU6hMCjMI75M1A2tKUQC";
        let uri = Some(format!("spotify:track:{id}"));
        assert_eq!(
            spotify_uri(&format!("https://open.spotify.com/track/{id}?si=abc")),
            uri
        );
        assert_eq!(
            spotify_uri(&format!("https://open.spotify.com/intl-id/track/{id}")),
            uri
        );
        assert_eq!(spotify_uri(&format!("spotify:track:{id}")), uri);
        assert_eq!(
            spotify_uri(&format!("https://open.spotify.com/playlist/{id}")),
            Some(format!("spotify:playlist:{id}"))
        );
        assert_eq!(spotify_uri("https://open.spotify.com/track/short"), None);
        assert_eq!(
            spotify_uri(&format!("https://open.spotify.com/user/{id}")),
            None
        );
        assert_eq!(
            spotify_uri("https://evil.example/track/4uLU6hMCjMI75M1A2tKUQC"),
            None
        );
    }
}
