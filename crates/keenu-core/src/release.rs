// SPDX-License-Identifier: GPL-3.0-or-later

//! Versions and the release manifest: the one document the updater, the install scripts and the
//! download page all read, so they cannot disagree about what exists.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

/// The only platforms a manifest can list.
pub const PLATFORMS: [&str; 6] = [
    "windows-x64",
    "windows-arm64",
    "macos-arm64",
    "macos-x64",
    "linux-x64",
    "linux-arm64",
];

/// A semantic version; build metadata is ignored, as the standard says it must be for ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}

impl Version {
    /// Reads `1.2.3`, `v1.2.3` or `1.2.3-rc.1+build`. `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let mut numbers = core.split('.');
        let number = |part: Option<&str>| -> Option<u64> {
            let part = part?;
            let valid = !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            valid.then(|| part.parse().ok()).flatten()
        };
        let (major, minor, patch) = (
            number(numbers.next())?,
            number(numbers.next())?,
            number(numbers.next())?,
        );
        if numbers.next().is_some() {
            return None;
        }
        let pre: Vec<String> = match pre {
            Some(pre) => {
                let parts: Vec<String> = pre.split('.').map(str::to_owned).collect();
                let valid = parts.iter().all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                });
                if !valid {
                    return None;
                }
                parts
            }
            None => Vec::new(),
        };
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let core =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if core != Ordering::Equal {
            return core;
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            // A release is newer than its prereleases.
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => compare_pre(&self.pre, &other.pre),
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_pre(a: &[String], b: &[String]) -> Ordering {
    for (a, b) in a.iter().zip(b) {
        let order = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            // Numbers sort before words.
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

/// Which stream of releases a release belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Prerelease,
}

/// Everything published for one version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub channel: Channel,
    pub release_notes_url: String,
    pub github_release_url: String,
    pub artifacts: Vec<Artifact>,
}

/// One file to download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// One of [`PLATFORMS`].
    pub platform: String,
    /// `msi`, `exe`, `zip`, `dmg`, `pkg`, `appimage`, `deb`, `rpm` or `tar.gz`.
    pub kind: String,
    pub file_name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub min_os: String,
    pub signature: Signature,
}

/// How a file is signed, as actually checked when the manifest was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    /// `authenticode`, `developer-id`, `gpg` or `none`.
    pub kind: String,
    pub signed: bool,
    #[serde(default)]
    pub signer: String,
}

/// The manifest: its releases, newest first. Its Ed25519 signature is a separate file beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub product: String,
    pub schema: u32,
    pub generated_at: String,
    pub releases: Vec<Release>,
}

impl Manifest {
    /// Errors that make a manifest unsafe to act on: anything else is a release the user can have.
    pub fn validate(&self) -> Result<(), String> {
        if self.product != "Keenu" {
            return Err(format!(
                "this manifest is for {:?}, not Keenu",
                self.product
            ));
        }
        for release in &self.releases {
            let version = Version::parse(&release.version)
                .ok_or_else(|| format!("{:?} is not a version", release.version))?;
            if version.is_prerelease() != (release.channel == Channel::Prerelease) {
                return Err(format!("{} is on the wrong channel", release.version));
            }
            for artifact in &release.artifacts {
                if !PLATFORMS.contains(&artifact.platform.as_str()) {
                    return Err(format!("{:?} is not a platform", artifact.platform));
                }
                if artifact.sha256.len() != 64
                    || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(format!("{} has no valid SHA-256", artifact.file_name));
                }
                if !secure_address(&artifact.url) {
                    return Err(format!("{} is not served over HTTPS", artifact.file_name));
                }
                if artifact.file_name.contains(['/', '\\']) || artifact.file_name.contains("..") {
                    return Err(format!("{:?} is not a file name", artifact.file_name));
                }
            }
        }
        Ok(())
    }

    /// The newest release on `channel` newer than `current` that has a file of `kinds` for
    /// `platform`, the kind listed first being preferred. The prerelease channel also sees stable ones.
    pub fn update_for(
        &self,
        current: &Version,
        channel: Channel,
        platform: &str,
        kinds: &[&str],
    ) -> Option<(&Release, &Artifact)> {
        let mut best: Option<(Version, &Release, &Artifact)> = None;
        for release in &self.releases {
            if channel == Channel::Stable && release.channel != Channel::Stable {
                continue;
            }
            let Some(version) = Version::parse(&release.version) else {
                continue;
            };
            if &version <= current || best.as_ref().is_some_and(|(top, ..)| &version <= top) {
                continue;
            }
            let artifact = kinds.iter().find_map(|kind| {
                release
                    .artifacts
                    .iter()
                    .find(|artifact| artifact.platform == platform && artifact.kind == *kind)
            });
            if let Some(artifact) = artifact {
                best = Some((version, release, artifact));
            }
        }
        best.map(|(_, release, artifact)| (release, artifact))
    }
}

/// HTTPS, or plain HTTP only to this very computer, as a release rehearsal does.
fn secure_address(url: &str) -> bool {
    if url.starts_with("https://") {
        return true;
    }
    url.strip_prefix("http://")
        .and_then(|rest| rest.split(['/', ':']).next())
        .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost"))
}

/// This build's platform as the manifest names it.
pub fn current_platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x64"),
        ("windows", "aarch64") => Some("windows-arm64"),
        ("macos", "aarch64") => Some("macos-arm64"),
        ("macos", "x86_64") => Some("macos-x64"),
        ("linux", "x86_64") => Some("linux-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn artifact(platform: &str, kind: &str) -> Artifact {
        Artifact {
            platform: platform.into(),
            kind: kind.into(),
            file_name: format!("keenu.{kind}"),
            url: format!("https://releases.invalid/keenu.{kind}"),
            sha256: "a".repeat(64),
            size: 1,
            min_os: String::new(),
            signature: Signature {
                kind: "none".into(),
                signed: false,
                signer: String::new(),
            },
        }
    }

    fn release(number: &str, artifacts: Vec<Artifact>) -> Release {
        Release {
            version: number.into(),
            channel: if version(number).is_prerelease() {
                Channel::Prerelease
            } else {
                Channel::Stable
            },
            release_notes_url: String::new(),
            github_release_url: String::new(),
            artifacts,
        }
    }

    fn manifest(releases: Vec<Release>) -> Manifest {
        Manifest {
            product: "Keenu".into(),
            schema: 1,
            generated_at: String::new(),
            releases,
        }
    }

    #[test]
    fn versions_order_as_semver_says() {
        let order = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.10.0",
            "2.0.0",
        ];
        for pair in order.windows(2) {
            assert!(version(pair[0]) < version(pair[1]), "{pair:?}");
        }
        assert_eq!(version("v1.2.3+build5"), version("1.2.3"));
    }

    #[test]
    fn rejects_what_is_not_a_version() {
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.x",
            "1.2.3-",
            "1.2.3-a..b",
            "a.b.c",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn picks_the_newest_for_this_platform_and_preferred_kind() {
        let manifest = manifest(vec![
            release(
                "1.1.0",
                vec![
                    artifact("windows-x64", "zip"),
                    artifact("windows-x64", "msi"),
                ],
            ),
            release("1.2.0", vec![artifact("linux-x64", "deb")]),
            release("1.0.0", vec![artifact("windows-x64", "msi")]),
        ]);
        let (found, file) = manifest
            .update_for(
                &version("1.0.0"),
                Channel::Stable,
                "windows-x64",
                &["msi", "zip"],
            )
            .unwrap();
        assert_eq!(
            (found.version.as_str(), file.kind.as_str()),
            ("1.1.0", "msi")
        );
    }

    #[test]
    fn nothing_when_up_to_date_and_never_a_downgrade() {
        let manifest = manifest(vec![release("1.0.0", vec![artifact("windows-x64", "msi")])]);
        for current in ["1.0.0", "1.0.1"] {
            assert!(
                manifest
                    .update_for(&version(current), Channel::Stable, "windows-x64", &["msi"])
                    .is_none()
            );
        }
    }

    #[test]
    fn stable_never_sees_a_prerelease_but_prerelease_sees_both() {
        let manifest = manifest(vec![
            release("2.0.0-rc.1", vec![artifact("linux-x64", "deb")]),
            release("1.5.0", vec![artifact("linux-x64", "deb")]),
        ]);
        let current = version("1.0.0");
        let pick = |channel| {
            manifest
                .update_for(&current, channel, "linux-x64", &["deb"])
                .map(|(release, _)| release.version.clone())
        };
        assert_eq!(pick(Channel::Stable).as_deref(), Some("1.5.0"));
        assert_eq!(pick(Channel::Prerelease).as_deref(), Some("2.0.0-rc.1"));
    }

    #[test]
    fn unsafe_manifests_are_rejected() {
        assert!(
            manifest(vec![release("1.0.0", vec![artifact("linux-x64", "deb")])])
                .validate()
                .is_ok()
        );

        let mut wrong = manifest(vec![]);
        wrong.product = "Other".into();
        assert!(wrong.validate().is_err());

        let bad = |change: fn(&mut Artifact)| {
            let mut file = artifact("linux-x64", "deb");
            change(&mut file);
            manifest(vec![release("1.0.0", vec![file])])
                .validate()
                .is_err()
        };
        assert!(bad(|file| file.platform = "plan9-x64".into()));
        assert!(bad(|file| file.sha256 = "abc".into()));
        assert!(bad(|file| file.sha256 = "z".repeat(64)));
        assert!(bad(
            |file| file.url = "http://releases.invalid/keenu.deb".into()
        ));
        assert!(bad(|file| file.file_name = "../evil.deb".into()));

        let mut channel = release("1.0.0-rc.1", vec![]);
        channel.channel = Channel::Stable;
        assert!(manifest(vec![channel]).validate().is_err());
    }
}
