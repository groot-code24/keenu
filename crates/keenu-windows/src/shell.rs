// SPDX-License-Identifier: GPL-3.0-or-later

//! The Windows shell: opening a file, showing it in Explorer, its thumbnail and starting at login.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use keenu_platform::{FileShelfProvider, Result, WindowLifecycleProvider};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
    REG_SAM_FLAGS, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::UI::Shell::{
    ILCreateFromPathW, ILFree, IShellItemImageFactory, QUNS_BUSY, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN, SHCreateItemFromParsingName, SHOpenFolderAndSelectItems,
    SHQueryUserNotificationState, SIIGBF_BIGGERSIZEOK, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_core::{HSTRING, PCWSTR};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "Keenu";

pub struct Shell;

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// A COM apartment for one call on this thread.
struct Com;

impl Com {
    fn new() -> Self {
        // SAFETY: paired with `CoUninitialize` in `Drop`; a thread already initialised just counts up.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: paired with `new`.
        unsafe { CoUninitialize() };
    }
}

/// An `HBITMAP`'s pixels as a PNG.
fn bitmap_to_png(bitmap: HBITMAP) -> Option<Vec<u8>> {
    // SAFETY: `bitmap` is valid; the buffer is sized from what Windows reports about it.
    unsafe {
        let mut info = BITMAP::default();
        let copied = GetObjectW(
            HGDIOBJ(bitmap.0),
            size_of::<BITMAP>() as i32,
            Some(std::ptr::from_mut(&mut info).cast()),
        );
        if copied == 0 || info.bmWidth <= 0 || info.bmHeight == 0 {
            return None;
        }
        let (width, height) = (info.bmWidth, info.bmHeight.abs());
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..BITMAPINFOHEADER::default()
            },
            ..BITMAPINFO::default()
        };
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        if lines == 0 {
            return None;
        }
        // Some thumbnails carry no alpha at all, though they are opaque.
        let opaque = pixels.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0);
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            if opaque {
                pixel[3] = 255;
            }
        }
        let mut png = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png);
        image::ImageEncoder::write_image(
            encoder,
            &pixels,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
        Some(png)
    }
}

impl FileShelfProvider for Shell {
    fn open(&self, path: &Path) -> Result<()> {
        let _com = Com::new();
        let file = wide_path(path);
        let verb = HSTRING::from("open");
        // SAFETY: both strings end in a NUL. Opening is what the user asked of this very file.
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(verb.as_ptr()),
                PCWSTR(file.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // Values above 32 mean success.
        if result.0 as isize > 32 {
            Ok(())
        } else {
            Err(format!("Windows could not open {}.", path.display()))
        }
    }

    fn reveal(&self, path: &Path) -> Result<()> {
        let _com = Com::new();
        let file = wide_path(path);
        // SAFETY: `file` ends in a NUL; the id list is freed below.
        unsafe {
            let list = ILCreateFromPathW(PCWSTR(file.as_ptr()));
            if list.is_null() {
                return Err(format!("{} was not found.", path.display()));
            }
            let result = SHOpenFolderAndSelectItems(list, None, 0);
            ILFree(Some(list));
            result.map_err(|err| format!("Explorer could not show it: {}", err.message()))
        }
    }

    fn thumbnail(&self, path: &Path, size: u32) -> Result<Option<Vec<u8>>> {
        let _com = Com::new();
        let file = wide_path(path);
        // SAFETY: `file` ends in a NUL; the bitmap is deleted after it is read.
        unsafe {
            let Ok(factory) = SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(
                PCWSTR(file.as_ptr()),
                None,
            ) else {
                return Ok(None);
            };
            let wanted = SIZE {
                cx: size as i32,
                cy: size as i32,
            };
            let Ok(bitmap) = factory.GetImage(wanted, SIIGBF_BIGGERSIZEOK) else {
                return Ok(None);
            };
            let png = bitmap_to_png(bitmap);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            Ok(png)
        }
    }
}

/// The key at `path`, or `None` if it is not there: a new profile may have no Run key until something adds one.
fn open_key(path: &str, access: REG_SAM_FLAGS) -> Result<Option<HKEY>> {
    let path = HSTRING::from(path);
    let mut key = HKEY::default();
    // SAFETY: closed by the caller.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            access,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(format!(
            "Could not open the startup settings ({}).",
            status.0
        ));
    }
    Ok(Some(key))
}

fn create_key(path: &str) -> Result<HKEY> {
    let path = HSTRING::from(path);
    let mut key = HKEY::default();
    // SAFETY: creates or opens a key under this user's hive, closed by the caller.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!(
            "Could not open the startup settings ({}).",
            status.0
        ));
    }
    Ok(key)
}

/// Sets or removes this program's value in the key at `path`. Removing from a key that is not there is already done.
fn set_startup_value(path: &str, enabled: bool) -> Result<()> {
    let key = if enabled {
        create_key(path)?
    } else {
        match open_key(path, KEY_SET_VALUE)? {
            Some(key) => key,
            None => return Ok(()),
        }
    };
    let name = HSTRING::from(RUN_VALUE);
    // SAFETY: `key` is open and closed below; strings end in a NUL.
    let status = unsafe {
        let status = if enabled {
            let exe = std::env::current_exe().map_err(|err| err.to_string())?;
            let value: Vec<u16> = format!("\"{}\"", exe.display())
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let bytes = std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), value.len() * 2);
            RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes))
        } else {
            match RegDeleteValueW(key, PCWSTR(name.as_ptr())) {
                status if status == ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                status => status,
            }
        };
        let _ = RegCloseKey(key);
        status
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "Could not change the startup setting ({}).",
            status.0
        ))
    }
}

fn startup_value_set(path: &str) -> Result<bool> {
    let Some(key) = open_key(path, KEY_QUERY_VALUE)? else {
        return Ok(false);
    };
    let name = HSTRING::from(RUN_VALUE);
    // SAFETY: only asks whether the value exists.
    let status = unsafe {
        let status = RegQueryValueExW(key, PCWSTR(name.as_ptr()), None, None, None, None);
        let _ = RegCloseKey(key);
        status
    };
    Ok(status == ERROR_SUCCESS)
}

impl WindowLifecycleProvider for Shell {
    fn set_start_at_login(&self, enabled: bool) -> Result<()> {
        set_startup_value(RUN_KEY, enabled)
    }

    fn start_at_login(&self) -> Result<bool> {
        startup_value_set(RUN_KEY)
    }

    fn fullscreen_active(&self) -> bool {
        // SAFETY: only reads the state.
        let state = unsafe { SHQueryUserNotificationState() };
        matches!(
            state,
            Ok(QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against the real registry, under this user: leaves it as it was.
    #[test]
    fn start_at_login_can_be_turned_on_and_off() {
        let before = Shell.start_at_login().unwrap();
        Shell.set_start_at_login(true).unwrap();
        assert!(Shell.start_at_login().unwrap());
        Shell.set_start_at_login(false).unwrap();
        assert!(!Shell.start_at_login().unwrap());
        Shell.set_start_at_login(false).unwrap();
        if before {
            Shell.set_start_at_login(true).unwrap();
        }
    }

    /// A profile can have no Run key at all: that is off, turning it off is done, and turning it on makes the key.
    #[test]
    fn a_startup_key_that_is_not_there_is_not_an_error() {
        let path = format!("Software\\Keenu-test-{}", std::process::id());
        assert!(!startup_value_set(&path).unwrap());
        set_startup_value(&path, false).unwrap();
        set_startup_value(&path, true).unwrap();
        assert!(startup_value_set(&path).unwrap());
        set_startup_value(&path, false).unwrap();
        assert!(!startup_value_set(&path).unwrap());
        let wide = HSTRING::from(path.as_str());
        // SAFETY: removes the empty key this test made.
        let status = unsafe {
            windows::Win32::System::Registry::RegDeleteKeyW(
                HKEY_CURRENT_USER,
                PCWSTR(wide.as_ptr()),
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
    }

    #[test]
    fn a_thumbnail_is_a_real_png_or_nothing() {
        let path = std::env::temp_dir().join(format!("keenu-thumb-{}.txt", std::process::id()));
        std::fs::write(&path, "text").unwrap();
        if let Some(png) = Shell.thumbnail(&path, 64).unwrap() {
            assert_eq!(&png[..4], b"\x89PNG");
        }
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            Shell
                .thumbnail(Path::new("C:\\no\\such\\file.x"), 64)
                .unwrap(),
            None
        );
    }

    #[test]
    fn revealing_or_opening_what_is_not_there_is_an_error() {
        assert!(Shell.reveal(Path::new("C:\\no\\such\\file.x")).is_err());
        assert!(Shell.open(Path::new("C:\\no\\such\\file.x")).is_err());
    }
}
