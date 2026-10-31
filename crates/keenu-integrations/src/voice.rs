// SPDX-License-Identifier: GPL-3.0-or-later

//! Speech and answers from an OpenAI-compatible API the user chose: the audio only goes where they
//! pointed Keenu, and the key comes from the credential store for each call.

use serde_json::{Value, json};

use crate::http;

/// An endpoint and how to reach it. `api_key` is empty for local servers that need none.
pub struct Endpoint<'a> {
    /// Such as `https://api.openai.com/v1`.
    pub base: &'a str,
    pub api_key: &'a str,
}

impl Endpoint<'_> {
    fn check(&self) -> Result<String, String> {
        if !http::is_secure_url(self.base) {
            return Err("The voice endpoint must use HTTPS, or be on this computer.".into());
        }
        Ok(self.base.trim_end_matches('/').to_owned())
    }

    fn authorization(&self) -> Option<String> {
        (!self.api_key.is_empty()).then(|| format!("Bearer {}", self.api_key))
    }
}

/// 16-bit mono PCM as a WAV file, what the transcription API takes.
pub fn wav(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// Sends a recording to `/audio/transcriptions`; returns the words.
pub fn transcribe(endpoint: &Endpoint, model: &str, wav_file: &[u8]) -> Result<String, String> {
    let url = format!("{}/audio/transcriptions", endpoint.check()?);
    let boundary = format!(
        "keenu{}",
        keenu_security::random_hex(12).map_err(|err| err.to_string())?
    );
    let mut body = Vec::with_capacity(wav_file.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{model}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav_file);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let content_type = format!("multipart/form-data; boundary={boundary}");
    let authorization = endpoint.authorization();
    let mut headers = vec![("Content-Type", content_type.as_str())];
    if let Some(authorization) = &authorization {
        headers.push(("Authorization", authorization));
    }
    let answer = http::send("POST", &url, &headers, Some(&body))
        .map_err(|err| format!("Could not reach the voice endpoint: {err}"))?;
    let reply = checked(&answer)?;
    reply["text"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "The voice endpoint sent no words.".to_owned())
}

/// Asks `/audio/speech` to say `text`; returns audio bytes.
pub fn speak(endpoint: &Endpoint, model: &str, voice: &str, text: &str) -> Result<Vec<u8>, String> {
    let url = format!("{}/audio/speech", endpoint.check()?);
    let body = json!({"model": model, "voice": voice, "input": text, "response_format": "wav"})
        .to_string();
    let authorization = endpoint.authorization();
    let mut headers = vec![("Content-Type", "application/json")];
    if let Some(authorization) = &authorization {
        headers.push(("Authorization", authorization));
    }
    let answer = http::send("POST", &url, &headers, Some(body.as_bytes()))
        .map_err(|err| format!("Could not reach the voice endpoint: {err}"))?;
    if !answer.is_success() {
        return Err(status_message(answer.status));
    }
    Ok(answer.body)
}

/// Asks `/chat/completions` for an answer to `question`.
pub fn answer(endpoint: &Endpoint, model: &str, question: &str) -> Result<String, String> {
    let url = format!("{}/chat/completions", endpoint.check()?);
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "You are Keenu, a concise desktop voice assistant. Keep spoken answers short."},
            {"role": "user", "content": question},
        ],
    })
    .to_string();
    let authorization = endpoint.authorization();
    let mut headers = vec![("Content-Type", "application/json")];
    if let Some(authorization) = &authorization {
        headers.push(("Authorization", authorization));
    }
    let answer = http::send("POST", &url, &headers, Some(body.as_bytes()))
        .map_err(|err| format!("Could not reach the voice endpoint: {err}"))?;
    let reply = checked(&answer)?;
    reply["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "The voice endpoint sent no answer.".to_owned())
}

fn checked(answer: &http::Response) -> Result<Value, String> {
    if !answer.is_success() {
        return Err(status_message(answer.status));
    }
    serde_json::from_slice(&answer.body)
        .map_err(|_| "The voice endpoint sent an unreadable answer.".to_owned())
}

/// Never repeats the body of an error answer, which can echo what was sent.
fn status_message(status: u16) -> String {
    match status {
        401 | 403 => "The voice endpoint refused the API key.".to_owned(),
        429 => "The voice endpoint says to slow down.".to_owned(),
        other => format!("The voice endpoint answered {other}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::test_server::serve;

    #[test]
    fn makes_a_valid_wav_header() {
        let file = wav(16_000, &[1, -1]);
        assert_eq!(&file[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(file[4..8].try_into().unwrap()), 36 + 4);
        assert_eq!(u32::from_le_bytes(file[24..28].try_into().unwrap()), 16_000);
        assert_eq!(&file[36..40], b"data");
        assert_eq!(file.len(), 44 + 4);
        assert_eq!(&file[44..46], &1i16.to_le_bytes());
    }

    #[test]
    fn audio_never_goes_to_an_insecure_address() {
        let endpoint = Endpoint {
            base: "http://voice.example.org/v1",
            api_key: "key",
        };
        assert!(transcribe(&endpoint, "m", b"x").is_err());
        assert!(speak(&endpoint, "m", "v", "x").is_err());
        assert!(answer(&endpoint, "m", "x").is_err());
    }

    #[test]
    fn sends_the_recording_and_reads_the_words() {
        let server = serve(vec![(200, vec![], r#"{"text":"hello there"}"#.into())]);
        let base = format!("{}/v1", server.url);
        let endpoint = Endpoint {
            base: &base,
            api_key: "sk-test",
        };
        assert_eq!(
            transcribe(&endpoint, "whisper-1", b"RIFFdata").unwrap(),
            "hello there"
        );
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[0].path, "/v1/audio/transcriptions");
        assert_eq!(seen[0].header("Authorization"), Some("Bearer sk-test"));
        assert!(seen[0].body.contains("name=\"model\"\r\n\r\nwhisper-1"));
        assert!(seen[0].body.contains("filename=\"speech.wav\""));
    }

    #[test]
    fn no_key_means_no_authorization_header() {
        let server = serve(vec![(
            200,
            vec![],
            r#"{"choices":[{"message":{"content":"Hi"}}]}"#.into(),
        )]);
        let endpoint = Endpoint {
            base: &server.url,
            api_key: "",
        };
        assert_eq!(answer(&endpoint, "m", "hello").unwrap(), "Hi");
        assert_eq!(server.seen.lock().unwrap()[0].header("Authorization"), None);
    }

    #[test]
    fn a_refused_key_says_so_without_repeating_the_answer() {
        let server = serve(vec![(
            401,
            vec![],
            r#"{"error":"bad key sk-secret"}"#.into(),
        )]);
        let endpoint = Endpoint {
            base: &server.url,
            api_key: "sk-secret",
        };
        let message = answer(&endpoint, "m", "hello").unwrap_err();
        assert!(message.contains("refused"));
        assert!(!message.contains("sk-secret"));
    }
}
