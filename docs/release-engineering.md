# Release engineering

How a Keenu release is made, and what it needs. The repository is
[groot-code24/keenu](https://github.com/groot-code24/keenu), and releases are published on its GitHub releases page.

## Setting up (once)

Most of this has working defaults, set in the workflows. These are the repository settings a release reads:

| Where | What | Needed for |
|---|---|---|
| `secrets.KEENU_UPDATE_SECRET_KEY` | The secret half of `keenu-release keygen`. The public half is in `scripts/install/install.sh` and the workflows | Signed manifests. **Releases refuse to run without it** |
| `vars.KEENU_PUBLISHER` | Your legal name, as on your certificate | MSI manufacturer. "Keenu contributors" until set |
| `secrets.KEENU_SIGNING_CERT_BASE64`, `KEENU_SIGNING_CERT_PASSWORD`, `vars.KEENU_SIGNING_THUMBPRINT` | A code-signing certificate (PFX) | Windows signing, and the updater. **Not set: installers are unsigned** |
| `vars.KEENU_REQUIRE_SIGNING = true` | | Refuses a stable release that would be unsigned |
| `vars.KEENU_BUNDLE_ID` | Another macOS identifier | `io.github.groot-code24.keenu` until set |
| `KEENU_SIGNING_IDENTITY`, `KEENU_NOTARY_PROFILE` | Developer ID and a notarization profile | macOS signing. **Not yet wired into the workflow** |
| `vars.KEENU_GOOGLE_CLIENT_ID`, `..._SECRET`, `vars.KEENU_MICROSOFT_CLIENT_ID` | See [calendar setup](calendar-setup.md) | Google and Microsoft sign-in |
| `vars.KEENU_MAINTAINER` | Your name and address | Linux packages. GitHub's no-reply address until set |

Keep a private backup of the update secret key: every later release must be signed with it. Use a protected GitHub
Environment for the publish job if others can push. To rotate a key, ship a release signed with the old one that holds
the new one.

## Making a release

1. Set the version in `Cargo.toml` and `gnome-extension/metadata.json`. Windows Installer takes numbers only, so
   `2.0.0-rc.1` is `2.0.0` in the MSI. **Do not ship two MSIs with the same three numbers and expect an upgrade.**
2. Move the changelog's `[Unreleased]` to `## [<version>] - <date>`.
3. Go through the [acceptance matrix](feature-acceptance-matrix.md). Hosted runners have one display and no microphone,
   so what needs those is for a person.
4. Commit and tag `v<version>`, then push the tag.

[release.yml](../.github/workflows/release.yml) then, in order:

1. Checks the tag against `Cargo.toml`, `metadata.json` and the changelog, and that the signing key is there.
2. Runs [windows.yml](../.github/workflows/windows.yml), [linux.yml](../.github/workflows/linux.yml) and
   [macos.yml](../.github/workflows/macos.yml), including real installs. Any failure stops everything.
3. Signs the Windows packages, if a certificate is set. Without one they stay unsigned, and the manifest and notes say so.
4. Makes the manifest from the files actually present, with their real checksums. It can only list what exists.
5. Signs the manifest and verifies that signature with the public key.
6. Attests provenance, and creates the GitHub release.
7. Commits the signed manifest to `website/` on `main`, where the website and the install scripts read it. Only now can
   anyone be offered this release, so a list never names a file that is not there.
8. Runs [verify-release.yml](../.github/workflows/verify-release.yml): downloads every published file and compares it,
   then runs the real one-command installers on clean Windows, macOS and Linux machines. Run it by hand from the Actions
   tab at any time.

Tags with a suffix (`v2.0.0-rc.1`) become prereleases. The installers and the website offer the newest stable release,
or the newest prerelease while there is no stable one.

## What gets published

| File | From | Notes |
|---|---|---|
| `Keenu-<v>-windows-x64.msi` | `scripts/windows/package.sh` (wixl) | Per-user, no admin. Tested in CI |
| `Keenu-<v>-windows-x64-setup.exe` | The same (NSIS) | Per-user, `/S` for silent. Tested in CI |
| `Keenu-<v>-windows-x64-portable.zip` | The same | |
| `keenu-<v>-linux-{x64,arm64}.tar.gz`, `.deb`, `.rpm` | `scripts/package.sh`, `scripts/package-system.sh` | Built, installed and tested in CI |
| `Keenu-<v>-macos-{arm64,x64}.dmg` | `packaging/macos/build-app.sh` | Installed and started in CI. Not notarized until set up |
| `keenu-release.json`, `.sig` | `keenu-release` | The manifest, also on `main` in `website/` |
| `SHA256SUMS` | `keenu-release checksums` | |
| `install.sh`, `install.ps1` | `scripts/install/` | Also on `main`, and on the website |

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

`website/` is static. [vercel.json](../vercel.json) deploys it on Vercel: import the repository at
[vercel.com/new](https://vercel.com/new), keep the settings it finds and the repository root, and deploy. Every push to
`main` then deploys again, including the commit that adds a release's manifest, so the page follows releases by itself.

[scripts/build-website.sh](../scripts/build-website.sh) puts the install scripts beside the page, so on your site they
are `https://<site>/install.sh` and `/install.ps1`, and the page shows those addresses. The same scripts also work from
GitHub with no site at all, and their release list comes from GitHub either way.

The page reads the manifest, recommends a file for the visitor's system, offers one for each system, and lists all of
them with checksums and signing status. With no manifest it says nothing is published yet: it cannot offer a file that
does not exist.

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
