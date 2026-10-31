// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Facts about this computer for the AI, which may not look for itself: the system, battery, disk and memory.

use std::fs;
use std::path::Path;
use std::process::Command;

const GB: f64 = 1024.0 * 1024.0 * 1024.0;

/// A few lines such as "Battery: 77%, not charging; charge limit 60% (…)", all on one line each.
pub fn facts() -> String {
    let mut lines = vec![format!("The system: {}.", system())];
    let managers = package_managers();
    if !managers.is_empty() {
        lines.push(format!(
            "Ways to install software here: {}.",
            managers.join(", ")
        ));
    }
    if let Some(folders) = folders() {
        lines.push(format!("Folders in ~: {folders}."));
    }
    lines.extend(battery());
    let mut usage = Vec::new();
    if let Some((free, total)) = disk() {
        usage.push(format!(
            "{} free of {} on the disk (/)",
            gb(free),
            gb(total)
        ));
    }
    if let Some((free, total)) = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|m| memory(&m))
    {
        usage.push(format!("{} of {} memory free", gb(free), gb(total)));
    }
    if !usage.is_empty() {
        lines.push(format!("Now: {}.", usage.join("; ")));
    }
    lines.join("\n")
}

/// The package managers installed, in the order an install should prefer them.
fn package_managers() -> Vec<&'static str> {
    let home = dirs::home_dir().unwrap_or_default();
    let dirs = [
        Path::new("/usr/bin").to_owned(),
        Path::new("/bin").to_owned(),
        Path::new("/usr/local/bin").to_owned(),
        home.join(".local/bin"),
    ];
    [
        "apt", "dnf", "zypper", "pacman", "flatpak", "snap", "brew", "npm", "pipx",
    ]
    .into_iter()
    .filter(|name| dirs.iter().any(|dir| dir.join(name).is_file()))
    .collect()
}

/// The folders in the home folder, not hidden, sorted; at most 40.
fn folders() -> Option<String> {
    let mut names: Vec<String> = fs::read_dir(dirs::home_dir()?)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names.truncate(40);
    (!names.is_empty()).then(|| names.join(", "))
}

/// A small file's trimmed text; `None` if missing or empty.
fn read(path: impl AsRef<Path>) -> Option<String> {
    let value = fs::read_to_string(path).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

/// "Ubuntu 24.04.5 LTS, GNOME, on a <vendor> <model>".
fn system() -> String {
    let os = read("/etc/os-release")
        .and_then(|release| {
            release.lines().find_map(|line| {
                let name = line.strip_prefix("PRETTY_NAME=")?;
                Some(name.trim_matches('"').to_owned())
            })
        })
        .unwrap_or_else(|| "Linux".into());
    let machine = ["sys_vendor", "product_name"]
        .iter()
        .filter_map(|name| read(format!("/sys/class/dmi/id/{name}")))
        .collect::<Vec<_>>()
        .join(" ");
    if machine.is_empty() {
        format!("{os}, GNOME")
    } else {
        format!("{os}, GNOME, on a {machine}")
    }
}

/// Each battery's charge and state, and its charge limit with the file that sets it, when it has one.
fn battery() -> Vec<String> {
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else {
        return Vec::new();
    };
    let mut batteries: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| read(path.join("type")).as_deref() == Some("Battery"))
        .collect();
    batteries.sort();
    batteries
        .iter()
        .map(|path| {
            let mut line = format!(
                "Battery: {}%, {}",
                read(path.join("capacity")).unwrap_or_else(|| "?".into()),
                read(path.join("status"))
                    .unwrap_or_else(|| "unknown".into())
                    .to_lowercase()
            );
            let limit = path.join("charge_control_end_threshold");
            let helper = charge_helper();
            match (read(&limit), helper) {
                // The helper keeps it after a restart, which writing the file alone does not.
                (Some(value), Some(helper)) => line.push_str(&format!(
                    "; charge limit {value}% (set it as root, kept after a restart, with the command \
                     `{helper} <percent>`; 100 removes it)"
                )),
                (Some(value), None) => line.push_str(&format!(
                    "; charge limit {value}% (set by writing to {} as root; resets on restart)",
                    limit.display()
                )),
                (None, _) => line.push_str("; no charge limit setting"),
            }
            line.push('.');
            line
        })
        .collect()
}

/// Free and total bytes on /, from `df`.
fn disk() -> Option<(f64, f64)> {
    let output = Command::new("df").args(["-Pk", "/"]).output().ok()?;
    df(&String::from_utf8_lossy(&output.stdout))
}

/// Available and total bytes from `df -Pk` output.
fn df(output: &str) -> Option<(f64, f64)> {
    let fields: Vec<&str> = output.lines().nth(1)?.split_whitespace().collect();
    let kib = |index: usize| fields.get(index)?.parse::<f64>().ok().map(|k| k * 1024.0);
    Some((kib(3)?, kib(1)?))
}

/// Available and total bytes, from /proc/meminfo.
fn memory(meminfo: &str) -> Option<(f64, f64)> {
    let field = |name: &str| {
        meminfo.lines().find_map(|line| {
            let kib = line.strip_prefix(name)?.trim().strip_suffix("kB")?;
            kib.trim().parse::<f64>().ok().map(|k| k * 1024.0)
        })
    };
    Some((field("MemAvailable:")?, field("MemTotal:")?))
}

/// "12.3 GB".
fn gb(bytes: f64) -> String {
    format!("{:.1} GB", bytes / GB)
}

/// How root runs Keenu's charge-limit helper: by name from a system package (/usr/bin), else by its whole
/// path from Keenu's own install in ~/.local/bin, which is not on root's PATH; `None` without it.
fn charge_helper() -> Option<String> {
    const NAME: &str = "keenu-charge-limit";
    if ["/usr/local/bin", "/usr/bin"]
        .iter()
        .any(|dir| Path::new(dir).join(NAME).is_file())
    {
        return Some(NAME.to_owned());
    }
    let own = dirs::home_dir()?.join(".local/bin").join(NAME);
    own.is_file().then(|| own.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_df() {
        let output = "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                      /dev/nvme0n1p2 490617784 256000000 209715200 56% /\n";
        let (free, total) = df(output).unwrap();
        assert_eq!(gb(free), "200.0 GB");
        assert_eq!(gb(total), "467.9 GB");
        assert_eq!(df("Filesystem\n"), None);
    }

    #[test]
    fn reads_meminfo() {
        let meminfo =
            "MemTotal:       16252928 kB\nMemFree:  1000 kB\nMemAvailable:    8126464 kB\n";
        let (free, total) = memory(meminfo).unwrap();
        assert_eq!((gb(free), gb(total)), ("7.8 GB".into(), "15.5 GB".into()));
    }
}
