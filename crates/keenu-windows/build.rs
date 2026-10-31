// SPDX-License-Identifier: GPL-3.0-or-later

//! Embeds the icon, the version and the manifest, and checks the release settings given to the build.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=keenu.manifest");
    println!("cargo:rerun-if-changed=../../packaging/icons/keenu.ico");
    for name in [
        "KEENU_SIGNING_THUMBPRINT",
        "KEENU_UPDATE_MANIFEST_URL",
        "KEENU_UPDATE_PUBLIC_KEY",
        "KEENU_GOOGLE_CLIENT_ID",
        "KEENU_GOOGLE_CLIENT_SECRET",
        "KEENU_MICROSOFT_CLIENT_ID",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if let Ok(value) = std::env::var("KEENU_SIGNING_THUMBPRINT") {
        let value: String = value.chars().filter(char::is_ascii_hexdigit).collect();
        if value.len() == 40 {
            println!(
                "cargo:rustc-env=KEENU_SIGNING_THUMBPRINT={}",
                value.to_ascii_uppercase()
            );
        } else if !value.is_empty() {
            println!(
                "cargo:warning=KEENU_SIGNING_THUMBPRINT is not a 40-character SHA-1 thumbprint and was ignored"
            );
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let numbers: Vec<u32> = version
        .split('-')
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    let (major, minor, patch) = (
        numbers.first().copied().unwrap_or(0),
        numbers.get(1).copied().unwrap_or(0),
        numbers.get(2).copied().unwrap_or(0),
    );
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let path = |relative: &str| {
        manifest_dir
            .join(relative)
            .display()
            .to_string()
            .replace('\\', "/")
    };
    let script = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "Keenu"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "keenu"
      VALUE "OriginalFilename", "keenu.exe"
      VALUE "ProductName", "Keenu"
      VALUE "ProductVersion", "{version}"
      VALUE "LegalCopyright", "GPL-3.0-or-later. Based on Isle by zickrian."
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#,
        icon = path("../../packaging/icons/keenu.ico"),
        manifest = path("keenu.manifest"),
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("keenu.rc");
    std::fs::write(&out, script).unwrap();
    // Without a resource compiler the program still works, only with a plain icon: not worth failing a
    // contributor's build, and the release workflow checks the result.
    let compiled = embed_resource::compile(&out, embed_resource::NONE);
    if let Err(err) = compiled.manifest_optional() {
        println!("cargo:warning=no resources embedded: {err}");
    }
}
