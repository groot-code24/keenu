// SPDX-License-Identifier: GPL-3.0-or-later

//! The Windows clipboard: text, rich text, images and files. Read only on a change while the user has
//! history on, and never when the copying app has asked to keep its content out of clipboard history, as
//! password managers do.

use std::path::PathBuf;

use keenu_core::clipboard::MAX_TEXT_BYTES;
use keenu_platform::{Availability, ClipboardContent, ClipboardProvider, Result};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::{BI_BITFIELDS, BI_RGB, BITMAPINFOHEADER};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::{DROPFILES, DragQueryFileW, HDROP};
use windows_core::{BOOL, HSTRING, PCWSTR};

/// An image bigger than this is left out of history: a screenshot fits, a poster does not.
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
/// Largest image side kept.
const MAX_SIDE: i32 = 16_384;

pub struct Clipboard;

/// The clipboard is shared: another program may hold it for a moment.
struct Opened;

impl Opened {
    fn new() -> Result<Self> {
        for _ in 0..10 {
            // SAFETY: closed again by `Drop`.
            if unsafe { OpenClipboard(Some(HWND::default())) }.is_ok() {
                return Ok(Self);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Err("Another program is holding the clipboard.".into())
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: opened by `new`.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn format_id(name: &str) -> u32 {
    let name = HSTRING::from(name);
    // SAFETY: `name` ends in a NUL; registering the same name always returns the same id.
    unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
}

/// Whether the copying app asked Windows to keep this out of clipboard history.
fn marked_private() -> bool {
    // SAFETY: only asks whether a format is on the clipboard, or reads a small value while it is open.
    unsafe {
        if IsClipboardFormatAvailable(format_id("ExcludeClipboardContentFromMonitorProcessing"))
            .is_ok()
        {
            return true;
        }
        // Its value is a DWORD: zero means "not in history".
        for name in ["CanIncludeInClipboardHistory"] {
            let id = format_id(name);
            if IsClipboardFormatAvailable(id).is_err() {
                continue;
            }
            let Ok(handle) = GetClipboardData(id) else {
                continue;
            };
            let global = HGLOBAL(handle.0);
            let pointer = GlobalLock(global);
            if !pointer.is_null() {
                let value = pointer.cast::<u32>().read_unaligned();
                let _ = GlobalUnlock(global);
                if value == 0 {
                    return true;
                }
            }
        }
    }
    false
}

/// A copy of a clipboard handle's bytes.
unsafe fn bytes_of(handle: HANDLE, limit: usize) -> Option<Vec<u8>> {
    let global = HGLOBAL(handle.0);
    // SAFETY: the caller got `handle` from the open clipboard, which owns it.
    unsafe {
        let size = GlobalSize(global);
        if size == 0 || size > limit {
            return None;
        }
        let pointer = GlobalLock(global);
        if pointer.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(global);
        Some(bytes)
    }
}

fn read_text() -> Option<String> {
    // SAFETY: called with the clipboard open.
    unsafe {
        IsClipboardFormatAvailable(u32::from(CF_UNICODETEXT.0)).ok()?;
        let handle = GetClipboardData(u32::from(CF_UNICODETEXT.0)).ok()?;
        let bytes = bytes_of(handle, MAX_TEXT_BYTES * 4)?;
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|&unit| unit != 0)
            .collect();
        Some(String::from_utf16_lossy(&units))
    }
}

fn read_rtf() -> Option<String> {
    let id = format_id("Rich Text Format");
    // SAFETY: called with the clipboard open.
    unsafe {
        IsClipboardFormatAvailable(id).ok()?;
        let bytes = bytes_of(GetClipboardData(id).ok()?, MAX_TEXT_BYTES)?;
        let end = bytes
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len());
        Some(String::from_utf8_lossy(&bytes[..end]).into_owned())
    }
}

fn read_files() -> Option<Vec<PathBuf>> {
    // SAFETY: called with the clipboard open; the drop handle belongs to the clipboard.
    unsafe {
        IsClipboardFormatAvailable(u32::from(CF_HDROP.0)).ok()?;
        let handle = GetClipboardData(u32::from(CF_HDROP.0)).ok()?;
        let drop = HDROP(handle.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        let mut files = Vec::new();
        for index in 0..count {
            let length = DragQueryFileW(drop, index, None) as usize;
            let mut buffer = vec![0u16; length + 1];
            let copied = DragQueryFileW(drop, index, Some(&mut buffer)) as usize;
            files.push(PathBuf::from(String::from_utf16_lossy(&buffer[..copied])));
        }
        (!files.is_empty()).then_some(files)
    }
}

/// A device-independent bitmap's pixels as a PNG.
pub fn dib_to_png(dib: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let header_size = size_of::<BITMAPINFOHEADER>();
    if dib.len() < header_size {
        return None;
    }
    // SAFETY: the length was checked; `read_unaligned` copes with the buffer's alignment.
    let header: BITMAPINFOHEADER =
        unsafe { dib.as_ptr().cast::<BITMAPINFOHEADER>().read_unaligned() };
    let (width, height) = (header.biWidth, header.biHeight);
    if width <= 0 || width > MAX_SIDE || height == 0 || height.abs() > MAX_SIDE {
        return None;
    }
    let bits = u32::from(header.biBitCount);
    if bits != 24 && bits != 32 {
        return None;
    }
    let compression = header.biCompression;
    if compression != BI_RGB.0 && compression != BI_BITFIELDS.0 {
        return None;
    }
    let masks = if compression == BI_BITFIELDS.0 && header.biSize as usize == header_size {
        12
    } else {
        0
    };
    let palette = header.biClrUsed as usize * 4;
    let pixels_at = (header.biSize as usize)
        .checked_add(masks)?
        .checked_add(palette)?;
    let stride = (width as usize * bits as usize).div_ceil(32) * 4;
    let rows = height.unsigned_abs() as usize;
    let needed = pixels_at.checked_add(stride.checked_mul(rows)?)?;
    if dib.len() < needed {
        return None;
    }
    // Screenshots leave alpha at zero throughout, though they are opaque.
    let bytes_per_pixel = bits as usize / 8;
    let has_alpha = bits == 32
        && (0..rows).any(|row| {
            dib[pixels_at + row * stride..pixels_at + row * stride + width as usize * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0)
        });
    let mut rgba = Vec::with_capacity(width as usize * rows * 4);
    for row in 0..rows {
        // Positive heights are stored bottom-up.
        let source_row = if height > 0 { rows - 1 - row } else { row };
        let start = pixels_at + source_row * stride;
        for pixel in
            dib[start..start + width as usize * bytes_per_pixel].chunks_exact(bytes_per_pixel)
        {
            rgba.extend_from_slice(&[
                pixel[2],
                pixel[1],
                pixel[0],
                if has_alpha { pixel[3] } else { 255 },
            ]);
        }
    }
    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png);
    image::ImageEncoder::write_image(
        encoder,
        &rgba,
        width as u32,
        rows as u32,
        image::ExtendedColorType::Rgba8,
    )
    .ok()?;
    Some((png, width as u32, rows as u32))
}

fn read_image() -> Option<ClipboardContent> {
    // SAFETY: called with the clipboard open.
    unsafe {
        IsClipboardFormatAvailable(u32::from(CF_DIB.0)).ok()?;
        let bytes = bytes_of(GetClipboardData(u32::from(CF_DIB.0)).ok()?, MAX_IMAGE_BYTES)?;
        let (png, width, height) = dib_to_png(&bytes)?;
        Some(ClipboardContent::Image { png, width, height })
    }
}

/// Puts `bytes` on the clipboard, which owns them from then on. Call with it open and emptied.
unsafe fn set_bytes(format: u32, bytes: &[u8]) -> Result<()> {
    // SAFETY: a fresh allocation, filled while locked; freed here unless the clipboard took it.
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|err| err.message())?;
        let pointer = GlobalLock(global);
        if pointer.is_null() {
            let _ = GlobalFree(Some(global));
            return Err("Out of memory.".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(global);
        if let Err(err) = SetClipboardData(format, Some(HANDLE(global.0))) {
            let _ = GlobalFree(Some(global));
            return Err(err.message());
        }
    }
    Ok(())
}

fn utf16_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// A list of files as `CF_HDROP` wants it.
fn dropfiles(files: &[PathBuf]) -> Vec<u8> {
    let header = DROPFILES {
        pFiles: size_of::<DROPFILES>() as u32,
        fWide: BOOL::from(true),
        ..DROPFILES::default()
    };
    // SAFETY: a plain structure read as its bytes.
    let mut bytes = unsafe {
        std::slice::from_raw_parts(
            std::ptr::from_ref(&header).cast::<u8>(),
            size_of::<DROPFILES>(),
        )
        .to_vec()
    };
    for file in files {
        bytes.extend(utf16_bytes(&file.to_string_lossy()));
    }
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

/// A PNG as the 32-bit bottom-up bitmap other programs paste.
fn png_to_dib(png: &[u8]) -> Result<Vec<u8>> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|err| err.to_string())?
        .to_rgba8();
    let (width, height) = image.dimensions();
    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: height as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..BITMAPINFOHEADER::default()
    };
    // SAFETY: a plain structure read as its bytes.
    let mut bytes = unsafe {
        std::slice::from_raw_parts(
            std::ptr::from_ref(&header).cast::<u8>(),
            size_of::<BITMAPINFOHEADER>(),
        )
        .to_vec()
    };
    for row in (0..height).rev() {
        for column in 0..width {
            let pixel = image.get_pixel(column, row).0;
            bytes.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    Ok(bytes)
}

impl ClipboardProvider for Clipboard {
    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn read(&self) -> Result<Option<ClipboardContent>> {
        let _open = Opened::new()?;
        if marked_private() {
            return Ok(None);
        }
        if let Some(files) = read_files() {
            return Ok(Some(ClipboardContent::Files(files)));
        }
        if let Some(text) = read_text() {
            return Ok(Some(match read_rtf() {
                Some(rtf) => ClipboardContent::Rich { text, rtf },
                None => ClipboardContent::Text(text),
            }));
        }
        Ok(read_image())
    }

    fn write(&self, content: &ClipboardContent) -> Result<()> {
        let _open = Opened::new()?;
        // SAFETY: the clipboard is open for the rest of this function.
        unsafe {
            EmptyClipboard().map_err(|err| err.message())?;
            match content {
                ClipboardContent::Text(text) => {
                    set_bytes(u32::from(CF_UNICODETEXT.0), &utf16_bytes(text))
                }
                ClipboardContent::Rich { text, rtf } => {
                    set_bytes(u32::from(CF_UNICODETEXT.0), &utf16_bytes(text))?;
                    let mut bytes = rtf.as_bytes().to_vec();
                    bytes.push(0);
                    set_bytes(format_id("Rich Text Format"), &bytes)
                }
                ClipboardContent::Image { png, .. } => {
                    set_bytes(u32::from(CF_DIB.0), &png_to_dib(png)?)
                }
                ClipboardContent::Files(files) => {
                    set_bytes(u32::from(CF_HDROP.0), &dropfiles(files))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Puts a 2 by 1 bitmap together: bottom-up, blue then red, in the given bit depth.
    fn dib(bits: u16, height: i32) -> Vec<u8> {
        let header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: 2,
            biHeight: height,
            biPlanes: 1,
            biBitCount: bits,
            biCompression: BI_RGB.0,
            ..BITMAPINFOHEADER::default()
        };
        let mut bytes = unsafe {
            std::slice::from_raw_parts(
                std::ptr::from_ref(&header).cast::<u8>(),
                size_of::<BITMAPINFOHEADER>(),
            )
            .to_vec()
        };
        if bits == 32 {
            bytes.extend_from_slice(&[255, 0, 0, 0, 0, 0, 255, 0]);
        } else {
            bytes.extend_from_slice(&[255, 0, 0, 0, 0, 255, 0, 0]);
        }
        bytes
    }

    fn decoded(png: &[u8]) -> image::RgbaImage {
        image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8()
    }

    #[test]
    fn screenshots_without_alpha_come_out_opaque_and_in_the_right_colours() {
        let (png, width, height) = dib_to_png(&dib(32, 1)).unwrap();
        assert_eq!((width, height), (2, 1));
        let image = decoded(&png);
        assert_eq!(image.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn reads_24_bit_bitmaps_with_their_row_padding() {
        let (png, ..) = dib_to_png(&dib(24, 1)).unwrap();
        assert_eq!(decoded(&png).get_pixel(1, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn malformed_bitmaps_are_refused_rather_than_read_past_their_end() {
        assert!(dib_to_png(&[]).is_none());
        let mut short = dib(32, 1);
        short.truncate(short.len() - 1);
        assert!(dib_to_png(&short).is_none());
        let mut huge = dib(32, 1);
        huge[4..8].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(dib_to_png(&huge).is_none());
        let mut negative = dib(32, 1);
        negative[4..8].copy_from_slice(&(-5i32).to_le_bytes());
        assert!(dib_to_png(&negative).is_none());
        assert!(dib_to_png(&dib(8, 1)).is_none(), "palettes are not kept");
    }

    #[test]
    fn a_pasted_image_round_trips() {
        let (png, ..) = dib_to_png(&dib(32, 1)).unwrap();
        let again = png_to_dib(&png).unwrap();
        let (png_again, ..) = dib_to_png(&again).unwrap();
        assert_eq!(decoded(&png), decoded(&png_again));
    }

    #[test]
    fn the_file_list_format_is_what_windows_expects() {
        let bytes = dropfiles(&[PathBuf::from("C:\\a"), PathBuf::from("C:\\b")]);
        let offset = size_of::<DROPFILES>();
        let units: Vec<u16> = bytes[offset..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(units.last(), Some(&0));
        assert_eq!(units[units.len() - 2], 0, "ends in two NULs");
        assert_eq!(String::from_utf16_lossy(&units[..3]), "C:\\");
    }
}
