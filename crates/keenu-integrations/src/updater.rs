// SPDX-License-Identifier: GPL-3.0-or-later

//! Finding and fetching an update, without trusting anything on the way: the manifest must carry a good
//! signature from the key built into this program, and each download must match the manifest's SHA-256.
//! Nothing is installed here; a host does that, after checking the platform's own signature as well.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use keenu_core::release::{Artifact, Channel, Manifest, Release, Version};
use keenu_security::integrity;

use crate::http;

/// Largest manifest accepted.
const MAX_MANIFEST: usize = 4 * 1024 * 1024;

/// Where this build looks for updates and whose signature it trusts, set when it is built. A build
/// without both cannot update, and says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateConfig {
    /// Address of `keenu-release.json`; its signature is beside it as `keenu-release.json.sig`.
    pub manifest_url: String,
    /// Base64 Ed25519 public key.
    pub public_key: String,
}

impl UpdateConfig {
    /// From `KEENU_UPDATE_MANIFEST_URL` and `KEENU_UPDATE_PUBLIC_KEY` as set at build time.
    pub fn built_in() -> Option<Self> {
        let manifest_url =
            option_env!("KEENU_UPDATE_MANIFEST_URL").filter(|url| !url.is_empty())?;
        let public_key = option_env!("KEENU_UPDATE_PUBLIC_KEY").filter(|key| !key.is_empty())?;
        Some(Self {
            manifest_url: manifest_url.to_owned(),
            public_key: public_key.to_owned(),
        })
    }
}

/// An update found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub version: String,
    pub notes_url: String,
    pub artifact: Artifact,
}

/// Fetches the manifest and checks its signature; nothing in it counts before that.
pub fn fetch_manifest(config: &UpdateConfig) -> Result<Manifest, String> {
    let body = get(&config.manifest_url)?;
    if body.len() > MAX_MANIFEST {
        return Err("The release manifest is too large.".into());
    }
    let signature = get(&format!("{}.sig", config.manifest_url))?;
    let signature = String::from_utf8(signature)
        .map_err(|_| "The manifest's signature is damaged.".to_owned())?;
    parse_signed(&body, &signature, &config.public_key)
}

/// Checks `bytes` against `signature`, then reads and validates them as a manifest.
pub fn parse_signed(bytes: &[u8], signature: &str, public_key: &str) -> Result<Manifest, String> {
    integrity::verify_signature(bytes, signature, public_key)
        .map_err(|why| format!("The release manifest failed its signature check: {why}."))?;
    let manifest: Manifest = serde_json::from_slice(bytes)
        .map_err(|_| "The release manifest cannot be read.".to_owned())?;
    manifest.validate()?;
    Ok(manifest)
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    if !http::is_secure_url(url) {
        return Err("The update address is not secure.".into());
    }
    let answer = http::send("GET", url, &[], None)
        .map_err(|err| format!("Could not look for updates: {err}"))?;
    if !answer.is_success() {
        return Err(format!("The update server answered {}.", answer.status));
    }
    Ok(answer.body)
}

/// The update for this build in `manifest`, if there is one.
pub fn find_update(
    manifest: &Manifest,
    current: &str,
    channel: Channel,
    platform: &str,
    kinds: &[&str],
) -> Result<Option<Update>, String> {
    let current = Version::parse(current).ok_or("This build's own version cannot be read.")?;
    Ok(manifest.update_for(&current, channel, platform, kinds).map(
        |(release, artifact): (&Release, &Artifact)| Update {
            version: release.version.clone(),
            notes_url: release.release_notes_url.clone(),
            artifact: artifact.clone(),
        },
    ))
}

/// Downloads `artifact` into `dir`, reporting `(bytes so far, total)`. The file gets its real name only
/// after its SHA-256 matches; a bad or partial one is deleted.
pub fn download(
    artifact: &Artifact,
    dir: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, String> {
    if !http::is_secure_url(&artifact.url) {
        return Err("The download address is not secure.".into());
    }
    std::fs::create_dir_all(dir).map_err(|err| format!("Could not prepare the download: {err}"))?;
    let target = dir.join(&artifact.file_name);
    let partial = dir.join(format!("{}.partial", artifact.file_name));
    let result = fetch_to(artifact, &partial, &mut progress);
    if let Err(why) = result {
        let _ = std::fs::remove_file(&partial);
        return Err(why);
    }
    if !integrity::file_matches(&partial, &artifact.sha256) {
        let _ = std::fs::remove_file(&partial);
        return Err("The download does not match the published checksum; it was deleted.".into());
    }
    std::fs::rename(&partial, &target)
        .map_err(|err| format!("Could not save the download: {err}"))?;
    Ok(target)
}

fn fetch_to(
    artifact: &Artifact,
    path: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(), String> {
    let mut answer = http::agent_for_downloads()
        .get(&artifact.url)
        .call()
        .map_err(|err| format!("Could not download the update: {err}"))?;
    let status = answer.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("The update server answered {status}."));
    }
    let mut reader = answer.body_mut().as_reader();
    let mut file =
        File::create(path).map_err(|err| format!("Could not save the download: {err}"))?;
    let mut buffer = [0u8; 64 * 1024];
    let mut done = 0u64;
    progress(0, artifact.size);
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| format!("The download failed: {err}"))?;
        if read == 0 {
            break;
        }
        done += read as u64;
        // A server sending more than was published is not sending what was published.
        if artifact.size > 0 && done > artifact.size {
            return Err("The download is larger than the published size.".into());
        }
        file.write_all(&buffer[..read])
            .map_err(|err| format!("Could not save the download: {err}"))?;
        progress(done, artifact.size);
    }
    file.sync_all()
        .map_err(|err| format!("Could not save the download: {err}"))
}

#[cfg(test)]
mod tests {
    use keenu_core::release::{Release, Signature};
    use keenu_security::integrity::{generate_keypair, sha256_bytes, sign};

    use super::*;
    use crate::calendar::test_server::serve;

    fn artifact(url: &str, sha256: &str, size: u64) -> Artifact {
        Artifact {
            platform: "windows-x64".into(),
            kind: "msi".into(),
            file_name: "Keenu-2.0.0.msi".into(),
            url: url.into(),
            sha256: sha256.into(),
            size,
            min_os: "10.0.19041".into(),
            signature: Signature {
                kind: "none".into(),
                signed: false,
                signer: String::new(),
            },
        }
    }

    fn manifest_bytes(url: &str) -> Vec<u8> {
        let manifest = Manifest {
            product: "Keenu".into(),
            schema: 1,
            generated_at: "2026-10-31T00:00:00Z".into(),
            releases: vec![Release {
                version: "2.0.0".into(),
                channel: Channel::Stable,
                release_notes_url: "https://notes.invalid".into(),
                github_release_url: String::new(),
                artifacts: vec![artifact(url, &"a".repeat(64), 1)],
            }],
        };
        serde_json::to_vec(&manifest).unwrap()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keenu-update-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_signed_manifest_is_read() {
        let (secret, public) = generate_keypair().unwrap();
        let bytes = manifest_bytes("https://releases.invalid/k.msi");
        let signature = sign(&bytes, &secret).unwrap();
        let manifest = parse_signed(&bytes, &signature, &public).unwrap();
        let update = find_update(&manifest, "1.5.0", Channel::Stable, "windows-x64", &["msi"])
            .unwrap()
            .unwrap();
        assert_eq!(update.version, "2.0.0");
    }

    #[test]
    fn a_changed_or_unsigned_manifest_is_refused() {
        let (secret, public) = generate_keypair().unwrap();
        let (other_secret, _) = generate_keypair().unwrap();
        let bytes = manifest_bytes("https://releases.invalid/k.msi");
        let signature = sign(&bytes, &secret).unwrap();
        let mut tampered = bytes.clone();
        tampered[20] ^= 1;
        assert!(parse_signed(&tampered, &signature, &public).is_err());
        assert!(parse_signed(&bytes, "", &public).is_err());
        let forged = sign(&bytes, &other_secret).unwrap();
        assert!(parse_signed(&bytes, &forged, &public).is_err());
    }

    #[test]
    fn a_signed_manifest_with_unsafe_content_is_still_refused() {
        let (secret, public) = generate_keypair().unwrap();
        let bytes = manifest_bytes("http://insecure.invalid/k.msi");
        let signature = sign(&bytes, &secret).unwrap();
        assert!(parse_signed(&bytes, &signature, &public).is_err());
    }

    #[test]
    fn nothing_when_this_build_is_current() {
        let (secret, public) = generate_keypair().unwrap();
        let bytes = manifest_bytes("https://releases.invalid/k.msi");
        let manifest = parse_signed(&bytes, &sign(&bytes, &secret).unwrap(), &public).unwrap();
        let found =
            find_update(&manifest, "2.0.0", Channel::Stable, "windows-x64", &["msi"]).unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn a_good_download_gets_its_name_and_reports_progress() {
        let body = "installer bytes";
        let server = serve(vec![(200, vec![], body.into())]);
        let file = artifact(
            &format!("{}/k.msi", server.url),
            &sha256_bytes(body.as_bytes()),
            body.len() as u64,
        );
        let dir = temp("good");
        let mut seen = Vec::new();
        let path = download(&file, &dir, |done, total| seen.push((done, total))).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        assert_eq!(seen.last(), Some(&(body.len() as u64, body.len() as u64)));
        assert!(!dir.join("Keenu-2.0.0.msi.partial").exists());
    }

    #[test]
    fn a_download_that_does_not_match_is_deleted() {
        let server = serve(vec![(200, vec![], "tampered".into())]);
        let file = artifact(
            &format!("{}/k.msi", server.url),
            &sha256_bytes(b"original"),
            0,
        );
        let dir = temp("bad");
        let result = download(&file, &dir, |_, _| {});
        assert!(result.unwrap_err().contains("checksum"));
        assert!(!dir.join("Keenu-2.0.0.msi").exists());
        assert!(!dir.join("Keenu-2.0.0.msi.partial").exists());
    }

    #[test]
    fn more_data_than_published_is_refused() {
        let server = serve(vec![(200, vec![], "much longer than published".into())]);
        let file = artifact(&format!("{}/k.msi", server.url), &"a".repeat(64), 4);
        let dir = temp("long");
        assert!(download(&file, &dir, |_, _| {}).is_err());
        assert!(!dir.join("Keenu-2.0.0.msi.partial").exists());
    }

    #[test]
    fn insecure_download_addresses_are_refused() {
        let file = artifact("http://evil.example/k.msi", &"a".repeat(64), 1);
        assert!(download(&file, &temp("insecure"), |_, _| {}).is_err());
    }
}
