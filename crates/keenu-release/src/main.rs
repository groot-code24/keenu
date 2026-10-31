// SPDX-License-Identifier: GPL-3.0-or-later

//! `keenu-release`: from a folder of built files to what a release publishes. Only files that exist are
//! listed, each with the checksum computed here, so the manifest cannot claim a download that is not there.

use std::collections::BTreeMap;
use std::path::Path;

use keenu_core::release::{Artifact, Channel, Manifest, Release, Signature, Version};
use keenu_security::integrity;

const HELP: &str = "keenu-release: makes Keenu's release manifest from the files actually built.

  keygen
      Prints a new signing key pair. Keep the secret in your release secrets.
  checksums <dist>
      Writes <dist>/SHA256SUMS.
  manifest <dist> --version <v> --base-url <url> --out <file> [options]
      Lists what is in <dist> under <url>. Options:
        --notes-url <url>            the release notes
        --github-url <url>           the GitHub release page
        --merge <file>               keeps the other releases of an earlier manifest
        --signed <kind>=<type>:<signer>   marks a kind (msi, exe, dmg...) as signed, once you have checked it
  sign <file> --key-env <VARIABLE>
      Writes <file>.sig with the base64 secret key in that environment variable.
  verify <file> --public-key <base64>
      Checks <file> against <file>.sig.
";

/// What each file name pattern is. Names come from `scripts/` and the release workflow.
fn classify(name: &str, version: &str) -> Option<(&'static str, &'static str)> {
    let rest = name
        .strip_prefix("Keenu-")
        .or_else(|| name.strip_prefix("keenu-"))?;
    let rest = rest.strip_prefix(version)?.strip_prefix('-')?;
    const KINDS: [(&str, &str); 9] = [
        ("-setup.exe", "exe"),
        (".msi", "msi"),
        ("-portable.zip", "zip"),
        (".dmg", "dmg"),
        (".pkg", "pkg"),
        (".AppImage", "appimage"),
        (".deb", "deb"),
        (".rpm", "rpm"),
        (".tar.gz", "tar.gz"),
    ];
    let (suffix, kind) = KINDS.iter().find(|(suffix, _)| rest.ends_with(suffix))?;
    let platform = rest.strip_suffix(suffix)?;
    let platform = keenu_core::release::PLATFORMS
        .iter()
        .find(|known| **known == platform)?;
    Some((platform, kind))
}

fn min_os(platform: &str) -> &'static str {
    match platform {
        "windows-x64" | "windows-arm64" => "Windows 10 version 2004 (19041) or Windows 11",
        "macos-arm64" | "macos-x64" => "macOS 11",
        _ => "GNOME Shell 46 to 51 on Linux",
    }
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|at| args.get(at + 1))
        .map(String::as_str)
}

fn required<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    option(args, name).ok_or_else(|| format!("{name} is required.\n\n{HELP}"))
}

/// `msi=authenticode:Publisher` into kind and signature.
fn parse_signed(text: &str) -> Result<(String, Signature), String> {
    let (kind, rest) = text
        .split_once('=')
        .ok_or("--signed takes kind=type:signer")?;
    let (signature, signer) = rest
        .split_once(':')
        .ok_or("--signed takes kind=type:signer")?;
    Ok((
        kind.to_owned(),
        Signature {
            kind: signature.to_owned(),
            signed: true,
            signer: signer.to_owned(),
        },
    ))
}

pub fn make_manifest(args: &[String]) -> Result<Manifest, String> {
    let dist = Path::new(
        args.first()
            .ok_or_else(|| format!("Say which folder.\n\n{HELP}"))?,
    );
    let version = required(args, "--version")?;
    let parsed = Version::parse(version).ok_or_else(|| format!("{version} is not a version."))?;
    let base = required(args, "--base-url")?
        .trim_end_matches('/')
        .to_owned();
    let mut signed = BTreeMap::new();
    for (at, arg) in args.iter().enumerate() {
        if arg == "--signed" {
            let (kind, signature) =
                parse_signed(args.get(at + 1).ok_or("--signed takes kind=type:signer")?)?;
            signed.insert(kind, signature);
        }
    }
    let mut artifacts = Vec::new();
    let entries = std::fs::read_dir(dist)
        .map_err(|err| format!("Could not read {}: {err}", dist.display()))?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        let Some((platform, kind)) = classify(&name, version) else {
            continue;
        };
        let path = dist.join(&name);
        let size = std::fs::metadata(&path)
            .map_err(|err| err.to_string())?
            .len();
        if size == 0 {
            return Err(format!("{name} is empty: not listing it."));
        }
        artifacts.push(Artifact {
            platform: platform.to_owned(),
            kind: kind.to_owned(),
            url: format!("{base}/{name}"),
            sha256: integrity::sha256_file(&path).map_err(|err| err.to_string())?,
            size,
            min_os: min_os(platform).to_owned(),
            signature: signed.get(kind).cloned().unwrap_or(Signature {
                kind: "none".into(),
                signed: false,
                signer: String::new(),
            }),
            file_name: name,
        });
    }
    if artifacts.is_empty() {
        return Err(format!(
            "{} holds no release files for {version}.",
            dist.display()
        ));
    }
    let release = Release {
        version: version.to_owned(),
        channel: if parsed.is_prerelease() {
            Channel::Prerelease
        } else {
            Channel::Stable
        },
        release_notes_url: option(args, "--notes-url").unwrap_or_default().to_owned(),
        github_release_url: option(args, "--github-url").unwrap_or_default().to_owned(),
        artifacts,
    };
    let mut releases = vec![release];
    if let Some(earlier) = option(args, "--merge") {
        let text = std::fs::read_to_string(earlier)
            .map_err(|err| format!("Could not read {earlier}: {err}"))?;
        let earlier: Manifest =
            serde_json::from_str(&text).map_err(|err| format!("{earlier}: {err}"))?;
        releases.extend(
            earlier
                .releases
                .into_iter()
                .filter(|old| old.version != version),
        );
    }
    let manifest = Manifest {
        product: "Keenu".into(),
        schema: 1,
        generated_at: chrono::Utc::now().to_rfc3339(),
        releases,
    };
    manifest.validate()?;
    Ok(manifest)
}

fn write_checksums(dist: &Path) -> Result<(), String> {
    let mut lines = Vec::new();
    let entries = std::fs::read_dir(dist).map_err(|err| err.to_string())?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "SHA256SUMS")
        .collect();
    names.sort();
    for name in names {
        let sum = integrity::sha256_file(&dist.join(&name)).map_err(|err| err.to_string())?;
        lines.push(format!("{sum}  {name}\n"));
    }
    std::fs::write(dist.join("SHA256SUMS"), lines.concat()).map_err(|err| err.to_string())
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let (secret, public) = integrity::generate_keypair()?;
            println!("secret: {secret}\npublic: {public}");
        }
        Some("checksums") => {
            write_checksums(Path::new(args.get(1).ok_or("Say which folder.")?))?;
        }
        Some("manifest") => {
            let manifest = make_manifest(&args[1..])?;
            let out = required(args, "--out")?;
            let text = serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?;
            std::fs::write(out, text).map_err(|err| format!("Could not write {out}: {err}"))?;
            let listed: Vec<String> = manifest.releases[0]
                .artifacts
                .iter()
                .map(|artifact| artifact.file_name.clone())
                .collect();
            println!("{out}: {}", listed.join(", "));
        }
        Some("sign") => {
            let file = args.get(1).ok_or("Say which file.")?;
            let variable = required(args, "--key-env")?;
            let secret = std::env::var(variable).map_err(|_| format!("{variable} is not set."))?;
            let bytes =
                std::fs::read(file).map_err(|err| format!("Could not read {file}: {err}"))?;
            let signature = integrity::sign(&bytes, &secret)?;
            std::fs::write(format!("{file}.sig"), signature).map_err(|err| err.to_string())?;
        }
        Some("verify") => {
            let file = args.get(1).ok_or("Say which file.")?;
            let public = required(args, "--public-key")?;
            let bytes =
                std::fs::read(file).map_err(|err| format!("Could not read {file}: {err}"))?;
            let signature = std::fs::read_to_string(format!("{file}.sig"))
                .map_err(|err| format!("No signature: {err}"))?;
            integrity::verify_signature(&bytes, &signature, public)?;
            println!("Good signature.");
        }
        _ => print!("{HELP}"),
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(why) = run(&args) {
        eprintln!("{why}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dist(name: &str, files: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("keenu-release-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for file in files {
            std::fs::write(dir.join(file), format!("contents of {file}")).unwrap();
        }
        dir
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn knows_each_files_platform_and_kind() {
        let v = "2.0.0";
        assert_eq!(
            classify("Keenu-2.0.0-windows-x64.msi", v),
            Some(("windows-x64", "msi"))
        );
        assert_eq!(
            classify("Keenu-2.0.0-windows-x64-setup.exe", v),
            Some(("windows-x64", "exe"))
        );
        assert_eq!(
            classify("Keenu-2.0.0-windows-x64-portable.zip", v),
            Some(("windows-x64", "zip"))
        );
        assert_eq!(
            classify("Keenu-2.0.0-macos-arm64.dmg", v),
            Some(("macos-arm64", "dmg"))
        );
        assert_eq!(
            classify("keenu-2.0.0-linux-x64.tar.gz", v),
            Some(("linux-x64", "tar.gz"))
        );
        assert_eq!(
            classify("keenu-2.0.0-linux-x64.deb", v),
            Some(("linux-x64", "deb"))
        );
        assert_eq!(
            classify("Keenu-1.9.0-windows-x64.msi", v),
            None,
            "another version"
        );
        assert_eq!(
            classify("Keenu-2.0.0-plan9-x64.msi", v),
            None,
            "no such platform"
        );
        assert_eq!(classify("SHA256SUMS", v), None);
    }

    #[test]
    fn lists_only_what_exists_with_its_real_checksum() {
        let dir = dist(
            "exists",
            &[
                "Keenu-2.0.0-windows-x64.msi",
                "notes.txt",
                "Keenu-1.0.0-windows-x64.msi",
            ],
        );
        let manifest = make_manifest(&strings(&[
            dir.to_str().unwrap(),
            "--version",
            "2.0.0",
            "--base-url",
            "https://releases.invalid/v2.0.0/",
        ]))
        .unwrap();
        let artifacts = &manifest.releases[0].artifacts;
        assert_eq!(artifacts.len(), 1);
        assert_eq!(
            artifacts[0].url,
            "https://releases.invalid/v2.0.0/Keenu-2.0.0-windows-x64.msi"
        );
        assert_eq!(
            artifacts[0].sha256,
            integrity::sha256_bytes(b"contents of Keenu-2.0.0-windows-x64.msi")
        );
        assert_eq!(artifacts[0].size, 39);
        assert!(
            !artifacts[0].signature.signed,
            "unsigned until said otherwise"
        );
        assert_eq!(manifest.releases[0].channel, Channel::Stable);
    }

    #[test]
    fn nothing_built_is_an_error_rather_than_an_empty_release() {
        let dir = dist("none", &["unrelated.txt"]);
        let result = make_manifest(&strings(&[
            dir.to_str().unwrap(),
            "--version",
            "2.0.0",
            "--base-url",
            "https://x.invalid",
        ]));
        assert!(result.is_err());
    }

    #[test]
    fn a_prerelease_goes_on_its_channel_and_signing_is_only_what_is_stated() {
        let dir = dist(
            "pre",
            &[
                "Keenu-2.0.0-rc.1-windows-x64.msi",
                "Keenu-2.0.0-rc.1-windows-x64-portable.zip",
            ],
        );
        let manifest = make_manifest(&strings(&[
            dir.to_str().unwrap(),
            "--version",
            "2.0.0-rc.1",
            "--base-url",
            "https://x.invalid",
            "--signed",
            "msi=authenticode:Example Publisher",
        ]))
        .unwrap();
        let release = &manifest.releases[0];
        assert_eq!(release.channel, Channel::Prerelease);
        let msi = release.artifacts.iter().find(|a| a.kind == "msi").unwrap();
        let zip = release.artifacts.iter().find(|a| a.kind == "zip").unwrap();
        assert!(msi.signature.signed);
        assert_eq!(msi.signature.signer, "Example Publisher");
        assert!(!zip.signature.signed);
    }

    #[test]
    fn an_insecure_base_address_is_refused() {
        let dir = dist("insecure", &["Keenu-2.0.0-windows-x64.msi"]);
        let result = make_manifest(&strings(&[
            dir.to_str().unwrap(),
            "--version",
            "2.0.0",
            "--base-url",
            "http://evil.example",
        ]));
        assert!(result.is_err());
    }

    #[test]
    fn checksums_match_what_sha256sum_would_say() {
        let dir = dist("sums", &["b.txt", "a.txt"]);
        write_checksums(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join("SHA256SUMS")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("  a.txt"));
        assert_eq!(
            lines[0].split_whitespace().next().unwrap(),
            integrity::sha256_bytes(b"contents of a.txt")
        );
    }
}
