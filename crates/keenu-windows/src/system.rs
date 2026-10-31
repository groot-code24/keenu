// SPDX-License-Identifier: GPL-3.0-or-later

//! Displays, the application in front, and what updating needs from Windows.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use keenu_platform::{
    Availability, Monitor, MonitorProvider, Result, UpdateProvider, UsageProvider,
};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::Security::Cryptography::{
    CERT_FIND_SUBJECT_CERT, CERT_HASH_PROP_ID, CERT_INFO,
    CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED, CERT_QUERY_FORMAT_FLAG_BINARY,
    CERT_QUERY_OBJECT_FILE, CMSG_SIGNER_INFO, CMSG_SIGNER_INFO_PARAM, CertCloseStore,
    CertFindCertificateInStore, CertFreeCertificateContext, CertGetCertificateContextProperty,
    CryptMsgClose, CryptMsgGetParam, CryptQueryObject, HCERTSTORE, PKCS_7_ASN_ENCODING,
    X509_ASN_ENCODING,
};
use windows::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CHOICE_FILE, WTD_REVOKE_WHOLECHAIN, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY,
    WTD_UI_NONE, WinVerifyTrust,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows_core::{PCWSTR, PWSTR};

const MONITORINFOF_PRIMARY: u32 = 1;

pub struct Displays;

/// A program of Windows' own by its full path: looked up by name, whatever is first on PATH would run.
pub fn system32(relative: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::path::Path::new(&root).join("System32").join(relative)
}

/// A monitor with where it may be used, not counting a taskbar along the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    pub monitor: Monitor,
    pub work_top: i32,
    pub dpi: u32,
}

unsafe extern "system" fn collect(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    list: LPARAM,
) -> windows_core::BOOL {
    // SAFETY: `list` is the vector passed to `EnumDisplayMonitors`, alive for the whole call.
    let list = unsafe { &mut *(list.0 as *mut Vec<Display>) };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `info` starts with a `MONITORINFO` of the size stated.
    let ok =
        unsafe { GetMonitorInfoW(monitor, std::ptr::from_mut(&mut info).cast::<MONITORINFO>()) };
    if ok.as_bool() {
        let mut dpi_x = 96;
        let mut dpi_y = 96;
        // SAFETY: only reads; on failure the defaults stand.
        let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let bounds = info.monitorInfo.rcMonitor;
        let name_end = info
            .szDevice
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(info.szDevice.len());
        list.push(Display {
            monitor: Monitor {
                name: String::from_utf16_lossy(&info.szDevice[..name_end]),
                is_primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                x: bounds.left,
                y: bounds.top,
                width: bounds.right - bounds.left,
                height: bounds.bottom - bounds.top,
                scale_percent: dpi_x * 100 / 96,
            },
            work_top: info.monitorInfo.rcWork.top,
            dpi: dpi_x,
        });
    }
    TRUE
}

/// Every display, the primary first and the rest from left to right.
pub fn displays() -> Vec<Display> {
    let mut list: Vec<Display> = Vec::new();
    // SAFETY: `collect` only runs during this call, while `list` is alive.
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(std::ptr::from_mut(&mut list) as isize),
        )
    };
    list.sort_by_key(|display| {
        (
            !display.monitor.is_primary,
            display.monitor.x,
            display.monitor.y,
        )
    });
    list
}

/// The display named `name`, or the primary when that one is gone.
pub fn choose_display<'a>(list: &'a [Display], name: &str) -> Option<&'a Display> {
    list.iter()
        .find(|display| !name.is_empty() && display.monitor.name == name)
        .or_else(|| list.iter().find(|display| display.monitor.is_primary))
        .or_else(|| list.first())
}

/// Where a window of `width` by `height` physical pixels goes: centred at the top of the display, below a
/// taskbar along the top. Negative coordinates are ordinary for a display left of or above the primary.
pub fn place(display: &Display, width: i32) -> (i32, i32) {
    (
        display.monitor.x + (display.monitor.width - width) / 2,
        display.work_top,
    )
}

impl MonitorProvider for Displays {
    fn monitors(&self) -> Result<Vec<Monitor>> {
        let list = displays();
        if list.is_empty() {
            return Err("Windows reports no displays.".into());
        }
        Ok(list.into_iter().map(|display| display.monitor).collect())
    }
}

pub struct Foreground;

impl UsageProvider for Foreground {
    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn idle_ms(&self) -> Option<u64> {
        let mut info = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: `info` has its size set; the tick count wraps along with `dwTime`.
        unsafe {
            GetLastInputInfo(&mut info).ok().ok()?;
            Some(u64::from(GetTickCount().wrapping_sub(info.dwTime)))
        }
    }

    /// The executable's name in lower case. Its window's title is never asked for.
    fn foreground_app(&self) -> Option<String> {
        // SAFETY: handles are closed below, and the buffer's length goes with it.
        unsafe {
            let window: HWND = GetForegroundWindow();
            if window.is_invalid() {
                return None;
            }
            let mut id = 0;
            GetWindowThreadProcessId(window, Some(&mut id));
            if id == 0 {
                return None;
            }
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, id).ok()?;
            let mut buffer = [0u16; 1024];
            let mut size = buffer.len() as u32;
            let result = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut size,
            );
            let _ = CloseHandle(process);
            result.ok()?;
            let full = String::from_utf16_lossy(&buffer[..size as usize]);
            Some(
                Path::new(&full)
                    .file_name()?
                    .to_string_lossy()
                    .to_lowercase(),
            )
        }
    }
}

/// Whether Windows considers a file's Authenticode signature valid, and it chains to a trusted root.
pub fn signature_valid(file: &Path) -> bool {
    let wide: Vec<u16> = file
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        ..WINTRUST_FILE_INFO::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut info },
        dwStateAction: WTD_STATEACTION_VERIFY,
        ..WINTRUST_DATA::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: the structures point at locals that outlive both calls; the second releases the first's state.
    unsafe {
        let status = WinVerifyTrust(
            HWND(usize::MAX as *mut _),
            &mut action,
            std::ptr::from_mut(&mut data).cast(),
        );
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        let _ = WinVerifyTrust(
            HWND(usize::MAX as *mut _),
            &mut action,
            std::ptr::from_mut(&mut data).cast(),
        );
        status == 0
    }
}

/// The SHA-1 thumbprint of the certificate that signed a file, in upper-case hex.
pub fn signer_thumbprint(file: &Path) -> Option<String> {
    let wide: Vec<u16> = file
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let encoding = X509_ASN_ENCODING | PKCS_7_ASN_ENCODING;
    // SAFETY: everything opened is closed below; buffers are sized by the calls that fill them.
    unsafe {
        let mut store = HCERTSTORE::default();
        let mut message: *mut core::ffi::c_void = std::ptr::null_mut();
        CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            wide.as_ptr().cast(),
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            None,
            None,
            None,
            Some(&mut store),
            Some(&mut message),
            None,
        )
        .ok()?;
        let result = (|| {
            let mut size = 0;
            CryptMsgGetParam(message, CMSG_SIGNER_INFO_PARAM, 0, None, &mut size).ok()?;
            let mut buffer = vec![0u8; size as usize];
            CryptMsgGetParam(
                message,
                CMSG_SIGNER_INFO_PARAM,
                0,
                Some(buffer.as_mut_ptr().cast()),
                &mut size,
            )
            .ok()?;
            let signer = &*buffer.as_ptr().cast::<CMSG_SIGNER_INFO>();
            let wanted = CERT_INFO {
                Issuer: signer.Issuer,
                SerialNumber: signer.SerialNumber,
                ..CERT_INFO::default()
            };
            let certificate = CertFindCertificateInStore(
                store,
                encoding,
                0,
                CERT_FIND_SUBJECT_CERT,
                Some(std::ptr::from_ref(&wanted).cast()),
                None,
            );
            if certificate.is_null() {
                return None;
            }
            let mut hash = [0u8; 20];
            let mut hash_size = hash.len() as u32;
            let got = CertGetCertificateContextProperty(
                certificate,
                CERT_HASH_PROP_ID,
                Some(hash.as_mut_ptr().cast()),
                &mut hash_size,
            );
            let _ = CertFreeCertificateContext(Some(certificate));
            got.ok()?;
            Some(keenu_security::hex(&hash[..hash_size as usize]).to_uppercase())
        })();
        let _ = CryptMsgClose(Some(message));
        let _ = CertCloseStore(Some(store), 0);
        result
    }
}

/// Installing updates on Windows: only an MSI, only one with a valid signature from the certificate this
/// build was told to trust. Windows Installer applies it as a transaction and rolls back if it fails.
pub struct Updates;

/// The publisher's certificate thumbprint, fixed when this build was made.
pub const fn pinned_thumbprint() -> Option<&'static str> {
    non_empty(option_env!("KEENU_SIGNING_THUMBPRINT"))
}

/// A build setting left empty, as a workflow passes one that is not set, is not set.
const fn non_empty(value: Option<&'static str>) -> Option<&'static str> {
    match value {
        Some(text) if !text.is_empty() => Some(text),
        _ => None,
    }
}

impl UpdateProvider for Updates {
    fn availability(&self) -> Availability {
        if keenu_integrations::updater::UpdateConfig::built_in().is_none() {
            return Availability::NotConfigured(
                "This build has no update address or key, so it cannot update itself.".into(),
            );
        }
        if pinned_thumbprint().is_none() {
            return Availability::NotConfigured(
                "This build was not made with a publisher certificate, so it cannot verify an update.".into(),
            );
        }
        Availability::Available
    }

    fn installable_kinds(&self) -> Vec<&'static str> {
        vec!["msi"]
    }

    fn verify_platform_signature(&self, file: &Path) -> Result<()> {
        verify_against(file, pinned_thumbprint())
    }

    fn install(&self, file: &Path) -> Result<()> {
        // Runs on after Keenu exits, so the installer can replace the program; /norestart leaves the
        // computer alone. Windows Installer rolls back by itself if it fails. Keenu then starts again,
        // the new version if it worked and the old one if not.
        use std::os::windows::process::CommandExt;
        let exe = std::env::current_exe().map_err(|err| err.to_string())?;
        let line = format!(
            "/S /C \"\"{}\" /i \"{}\" /passive /norestart & \"{}\"\"",
            system32("msiexec.exe").display(),
            file.display(),
            exe.display()
        );
        std::process::Command::new(system32("cmd.exe"))
            .raw_arg(line)
            // No console window.
            .creation_flags(0x0800_0000)
            .spawn()
            .map(drop)
            .map_err(|err| format!("Could not start Windows Installer: {err}"))
    }
}

/// Checks a file is validly signed, by exactly the pinned certificate.
pub fn verify_against(file: &Path, pinned: Option<&str>) -> Result<()> {
    let Some(pinned) = pinned else {
        return Err(
            "No publisher certificate is pinned in this build; the update was not installed."
                .into(),
        );
    };
    if !signature_valid(file) {
        return Err("The update's signature is not valid; it was not installed.".into());
    }
    match signer_thumbprint(file) {
        Some(found) if keenu_security::constant_time_eq(found.as_bytes(), pinned.as_bytes()) => {
            Ok(())
        }
        _ => Err("The update was signed by someone else; it was not installed.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(name: &str, primary: bool, x: i32, y: i32, width: i32, work_top: i32) -> Display {
        Display {
            monitor: Monitor {
                name: name.into(),
                is_primary: primary,
                x,
                y,
                width,
                height: 1080,
                scale_percent: 100,
            },
            work_top,
            dpi: 96,
        }
    }

    #[test]
    fn a_window_is_centred_even_on_a_display_left_of_the_primary() {
        let left = display("L", false, -1920, 0, 1920, 0);
        assert_eq!(place(&left, 232), (-1920 + 844, 0));
        let above = display("A", false, 0, -1080, 1920, -1080);
        assert_eq!(place(&above, 232), (844, -1080));
    }

    #[test]
    fn a_taskbar_along_the_top_is_left_clear() {
        let top_bar = display("P", true, 0, 0, 1920, 48);
        assert_eq!(place(&top_bar, 232).1, 48);
    }

    #[test]
    fn a_chosen_display_that_has_gone_falls_back_to_the_primary() {
        let list = vec![
            display("B", false, 1920, 0, 1920, 0),
            display("A", true, 0, 0, 1920, 0),
        ];
        assert_eq!(choose_display(&list, "B").unwrap().monitor.name, "B");
        assert_eq!(choose_display(&list, "gone").unwrap().monitor.name, "A");
        assert_eq!(choose_display(&list, "").unwrap().monitor.name, "A");
        assert!(choose_display(&[], "x").is_none());
    }

    /// Against the real display list. CI runners have one too.
    #[test]
    fn windows_lists_its_displays() {
        let list = displays();
        assert!(!list.is_empty());
        assert!(list[0].monitor.is_primary);
        assert!(list[0].monitor.scale_percent >= 100);
    }

    #[test]
    fn the_foreground_app_is_a_bare_file_name_when_there_is_one() {
        if let Some(name) = Foreground.foreground_app() {
            assert!(!name.contains('\\') && !name.contains('/'), "{name}");
            assert_eq!(name, name.to_lowercase());
        }
    }

    #[test]
    fn an_unsigned_file_is_refused() {
        let path = std::env::temp_dir().join(format!("keenu-unsigned-{}.msi", std::process::id()));
        std::fs::write(&path, b"not really an installer").unwrap();
        assert!(!signature_valid(&path));
        assert_eq!(signer_thumbprint(&path), None);
        assert!(verify_against(&path, Some(&"A".repeat(40))).is_err());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn with_nothing_pinned_nothing_can_be_installed() {
        let result = verify_against(Path::new("C:\\Windows\\explorer.exe"), None);
        assert!(result.unwrap_err().contains("No publisher certificate"));
    }

    #[test]
    fn a_setting_left_empty_pins_nothing() {
        assert_eq!(non_empty(Some("")), None);
        assert_eq!(non_empty(None), None);
        assert_eq!(non_empty(Some("ab")), Some("ab"));
    }

    #[test]
    fn a_this_build_without_release_settings_says_it_cannot_update() {
        if keenu_integrations::updater::UpdateConfig::built_in().is_none() {
            assert!(matches!(
                Updates.availability(),
                Availability::NotConfigured(_)
            ));
        }
    }
}
