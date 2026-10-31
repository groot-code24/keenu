# Release engineering

How a Keenu release is made, and what has to be set up first. **No release has been published yet.** Parts marked
_rehearsed_ have run for real on one machine; the workflows have not yet run on GitHub.

## Setting up (once)

Keenu does not assume who publishes it. Nothing below is invented in the source; each is a placeholder until you
provide it.

| Where | What | Needed for |
|---|---|---|
| Replace `YOUR-GITHUB-OWNER` across the repository | The GitHub owner | Links, `gh attestation verify` |
| Replace `YOUR-KEENU-DOMAIN` in `website/` and `scripts/install/` | Where the page and scripts are served | One-command install |
| `vars.KEENU_RELEASE_BASE_URL` | Where files are downloaded from: `https://<host>/releases`, one folder per `v<version>` | The manifest |
| `vars.KEENU_UPDATE_MANIFEST_URL` | Address of `keenu-release.json` | The updater, install scripts |
| `vars.KEENU_UPDATE_PUBLIC_KEY`, `secrets.KEENU_UPDATE_SECRET_KEY` | From `keenu-release keygen` | Signed manifests. **Releases refuse to run without these** |
| `vars.KEENU_PUBLISHER` | Your legal name, as on your certificate | MSI manufacturer |
| `secrets.KEENU_SIGNING_CERT_BASE64`, `KEENU_SIGNING_CERT_PASSWORD`, `vars.KEENU_SIGNING_THUMBPRINT` | A code-signing certificate (PFX) | Windows signing, and the updater |
| `vars.KEENU_REQUIRE_SIGNING = true` | | Refuses a stable release that would be unsigned |
| `vars.KEENU_BUNDLE_ID` | Your reverse-domain identifier | macOS |
| `KEENU_SIGNING_IDENTITY`, `KEENU_NOTARY_PROFILE` | Developer ID and a notarization profile | macOS signing. **Not yet wired into the workflow** |
| `vars.KEENU_GOOGLE_CLIENT_ID`, `..._SECRET`, `vars.KEENU_MICROSOFT_CLIENT_ID` | See [calendar setup](calendar-setup.md) | Google and Microsoft sign-in |
| `vars.KEENU_RELEASE_REPOSITORY` at build time | `owner/name` | Linux's "a new version is out" |
| `MAINTAINER` in `scripts/package-system.sh` through `KEENU_MAINTAINER`, and `packaging/aur/PKGBUILD` | Your name and address | Linux packages |

Keep the update secret key offline if you can, and use a protected GitHub Environment for the publish job. To rotate
a key, ship a release signed with the old one that holds the new one.

## Making a release

1. Set the version in `Cargo.toml` and `gnome-extension/metadata.json`. Windows Installer takes numbers only, so
   `2.0.0-rc.1` is `2.0.0` in the MSI. **Do not ship two MSIs with the same three numbers and expect an upgrade.**
2. Move the changelog's `[Unreleased]` to `## [<version>] - <date>`.
3. Go through the [acceptance matrix](feature-acceptance-matrix.md). Hosted runners have one display and no microphone,
   so what needs those is for a person.
4. Commit and tag `v<version>`, then push the tag.

[release.yml](../.github/workflows/release.yml) then, in order:

1. Checks the tag against `Cargo.toml`, `metadata.json` and the changelog, and that the settings are there.
2. Runs [windows.yml](../.github/workflows/windows.yml), [linux.yml](../.github/workflows/linux.yml) and
   [macos.yml](../.github/workflows/macos.yml), including real installs. Any failure stops everything.
3. Signs the Windows packages, if a certificate is set. Without one they stay unsigned, and the manifest and notes say so.
4. Makes the manifest from the files actually present, with their real checksums. It can only list what exists.
5. Signs the manifest and verifies that signature with the public key.
6. Fills the addresses into the install scripts.
7. Attests provenance, and creates the GitHub release.

Tags with a suffix (`v2.0.0-rc.1`) become prereleases, and go on the prerelease channel.

## What gets published

| File | From | Notes |
|---|---|---|
| `Keenu-<v>-windows-x64.msi` | `scripts/windows/package.sh` (wixl) | Per-user, no admin. _Rehearsed_ |
| `Keenu-<v>-windows-x64-setup.exe` | The same (NSIS) | Per-user, `/S` for silent. _Rehearsed_ |
| `Keenu-<v>-windows-x64-portable.zip` | The same | _Rehearsed_ |
| `keenu-<v>-linux-{x64,arm64}.tar.gz`, `.deb`, `.rpm` | `scripts/package.sh`, `scripts/package-system.sh` | _Rehearsed_ on x64, without Vulkan: it needs more memory than was available |
| `Keenu-<v>-macos-{arm64,x64}.dmg` | `packaging/macos/build-app.sh` | Never run. Not notarized until set up |
| `keenu-release.json`, `.sig` | `keenu-release` | The manifest. _Rehearsed_ |
| `SHA256SUMS` | `keenu-release checksums` | |
| `install.sh`, `install.ps1` | `scripts/install/` | _Rehearsed against a server on this computer_ |

**Not offered:** an AppImage, because on Linux Keenu is a GNOME Shell extension, which has to be installed into your
home folder or the system, and an AppImage cannot do that. A signed `.pkg` for macOS, until there is a notarized `.dmg`.
A Windows ARM64 installer: it builds in CI but `package.sh` makes x64 MSIs, and it has not run.

## The manifest

```json
{
  "product": "Keenu",
  "schema": 1,
  "generated_at": "2026-10-31T00:00:00Z",
  "releases": [{
    "version": "2.0.0",
    "channel": "stable",
    "release_notes_url": "https://...",
    "github_release_url": "https://...",
    "artifacts": [{
      "platform": "windows-x64",
      "kind": "msi",
      "file_name": "Keenu-2.0.0-windows-x64.msi",
      "url": "https://.../Keenu-2.0.0-windows-x64.msi",
      "sha256": "...",
      "size": 1736704,
      "min_os": "Windows 10 version 2004 (19041) or Windows 11",
      "signature": { "kind": "authenticode", "signed": true, "signer": "..." }
    }]
  }]
}
```

Keep older releases in it with `--merge`, so nobody on the prerelease channel is left behind. Platforms are limited to
`windows-x64`, `windows-arm64`, `macos-arm64`, `macos-x64`, `linux-x64` and `linux-arm64`.

## The website

`website/` is static, with no build step. Serve it next to `keenu-release.json`. It reads the manifest, recommends a
file for the visitor's system, and lists the rest with checksums and signing status. With no manifest it says nothing
is published yet: it cannot offer a file that does not exist.

Replace `YOUR-KEENU-DOMAIN` and `YOUR-GITHUB-OWNER` in `index.html` first.

## Reproducing the builds

With only Docker, on any system:

```sh
docker build -t keenu-windows-cross -f packaging/docker/windows-cross.Dockerfile packaging/docker
docker run --rm -v "$PWD:/src" -v keenu-cross-target:/target -w /src keenu-windows-cross bash -c \
  'cargo build --release --locked --target x86_64-pc-windows-gnu -p keenu-windows &&
   cp /target/x86_64-pc-windows-gnu/release/keenu.exe dist/ &&
   bash scripts/windows/package.sh dist/keenu.exe'

docker build -t keenu-linux-build -f packaging/docker/linux-build.Dockerfile packaging/docker
docker run --rm -v "$PWD:/src" -v keenu-linux-target:/target -e CARGO_TARGET_DIR=/target -w /src keenu-linux-build \
  bash -c 'tarball="$(scripts/package.sh | tail -n1)" && scripts/package-system.sh "$tarball"'
```

The Linux build needs more than 3.5 GB of memory: whisper.cpp's Vulkan shaders do not compile in that. If Docker has too
little, add `--memory` with a little less than it has to `docker run`, so that running out ends the build and not
Docker, with everything else running in it. On one 3.5 GB machine, leaving it out stopped Docker's engine.

The Windows build through Docker uses mingw-w64, while official releases use MSVC. Rust's results differ between them,
which is why CI tests the MSVC one. Linux packages built in `rust:1-trixie` need its glibc: CI builds on Ubuntu 24.04
so they run on older systems too.

## Testing a package by hand

```powershell
python scripts\windows\installer_test.py --msi ... --old-msi ... --setup ... --zip ... --release-tool ... --version ...
```

It installs, upgrades, removes and tampers for real, under your user, so run it on a machine without Keenu installed.
Some antivirus products flagged a PowerShell version of this test as malicious on its behaviour, which is why it is in
Python. Before a release, check `install.ps1` and the signed installers with the antivirus products you can, and report
false positives to their vendors, rather than changing anything to get past them.
