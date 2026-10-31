// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks that a download is what was published: its SHA-256, and the release manifest's Ed25519
//! signature. Nothing downloaded is ever opened before these pass.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

/// The file's SHA-256, as lower-case hex.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer)? {
            0 => return Ok(crate::hex(&hasher.finalize())),
            read => hasher.update(&buffer[..read]),
        }
    }
}

/// `bytes`' SHA-256, as lower-case hex.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    crate::hex(&Sha256::digest(bytes))
}

/// The PKCE challenge of an OAuth `verifier` (RFC 7636, S256).
pub fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Whether the file has this SHA-256; an unreadable file does not.
pub fn file_matches(path: &Path, expected: &str) -> bool {
    sha256_file(path).is_ok_and(|actual| {
        crate::constant_time_eq(actual.as_bytes(), expected.to_ascii_lowercase().as_bytes())
    })
}

/// Checks `message` against a base64 signature and public key. Malformed input fails like a bad signature.
pub fn verify_signature(
    message: &[u8],
    signature_base64: &str,
    public_key_base64: &str,
) -> Result<(), String> {
    let key: [u8; 32] = STANDARD
        .decode(public_key_base64.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or("the public key is malformed")?;
    let key = VerifyingKey::from_bytes(&key).map_err(|_| "the public key is not valid")?;
    let signature: [u8; 64] = STANDARD
        .decode(signature_base64.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or("the signature is malformed")?;
    key.verify_strict(message, &Signature::from_bytes(&signature))
        .map_err(|_| "the signature does not match".to_owned())
}

/// A new signing key: `(secret, public)`, base64.
pub fn generate_keypair() -> Result<(String, String), String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|err| format!("no secure random numbers: {err}"))?;
    let key = SigningKey::from_bytes(&seed);
    Ok((
        STANDARD.encode(key.to_bytes()),
        STANDARD.encode(key.verifying_key().to_bytes()),
    ))
}

/// Signs `message` with a base64 secret key, giving a base64 signature.
pub fn sign(message: &[u8], secret_base64: &str) -> Result<String, String> {
    let secret: [u8; 32] = STANDARD
        .decode(secret_base64.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or("the signing key is malformed")?;
    let key = SigningKey::from_bytes(&secret);
    Ok(STANDARD.encode(key.sign(message).to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_known_values() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn pkce_matches_the_example_in_its_rfc() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_file_is_checked_by_its_contents() {
        let path = std::env::temp_dir().join(format!("keenu-hash-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        let right = "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";
        assert!(file_matches(&path, right));
        assert!(!file_matches(&path, &"0".repeat(64)));
        std::fs::write(&path, b"abd").unwrap();
        assert!(!file_matches(&path, right), "a changed file fails");
        std::fs::remove_file(&path).unwrap();
        assert!(!file_matches(&path, right), "a missing file fails");
    }

    #[test]
    fn a_good_signature_verifies() {
        let (secret, public) = generate_keypair().unwrap();
        let signature = sign(b"manifest", &secret).unwrap();
        assert_eq!(verify_signature(b"manifest", &signature, &public), Ok(()));
    }

    #[test]
    fn anything_else_fails() {
        let (secret, public) = generate_keypair().unwrap();
        let (_, other) = generate_keypair().unwrap();
        let signature = sign(b"manifest", &secret).unwrap();
        assert!(
            verify_signature(b"manifest!", &signature, &public).is_err(),
            "changed"
        );
        assert!(
            verify_signature(b"manifest", &signature, &other).is_err(),
            "other key"
        );
        assert!(verify_signature(b"manifest", "not base64!", &public).is_err());
        assert!(verify_signature(b"manifest", "AAAA", &public).is_err());
        assert!(verify_signature(b"manifest", &signature, "").is_err());
        let zeros = STANDARD.encode([0u8; 32]);
        assert!(
            verify_signature(b"manifest", &signature, &zeros).is_err(),
            "weak key"
        );
    }
}
