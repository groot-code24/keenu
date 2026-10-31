// SPDX-License-Identifier: GPL-3.0-or-later

//! Paints a scene with Direct2D and DirectWrite into a bitmap with real transparency, for a layered window:
//! the island's rounded corners blend with whatever is behind them.

use std::collections::HashMap;
use std::sync::Arc;

use keenu_ui::icons::{Icon, polygons};
use keenu_ui::scene::{Align, Color, Op, Rect, Scene, TextStyle};
use windows::Win32::Foundation::{HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_END_CLOSED, D2D1_FILL_MODE_ALTERNATE, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Bitmap,
    ID2D1DCRenderTarget, ID2D1Factory, ID2D1PathGeometry,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP,
    HDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};
use windows_core::{Result, w};
use windows_numerics::{Matrix3x2, Vector2};

/// A memory bitmap Direct2D draws into and the window shows.
pub struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    pixels: *mut u8,
    pub width: i32,
    pub height: i32,
}

impl Surface {
    pub fn new(width: i32, height: i32) -> Result<Self> {
        // SAFETY: handles are kept here and released in `Drop`; `pixels` lives as long as the bitmap.
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
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
            let mut pixels = std::ptr::null_mut();
            let bitmap =
                CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut pixels, None, 0);
            ReleaseDC(None, screen);
            let bitmap = bitmap?;
            let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
            Ok(Self {
                dc,
                bitmap,
                previous,
                pixels: pixels.cast(),
                width,
                height,
            })
        }
    }

    /// Shows this bitmap as `window`, at `(x, y)` on the screen.
    pub fn present(&self, window: HWND, x: i32, y: i32) -> Result<()> {
        let position = POINT { x, y };
        let size = SIZE {
            cx: self.width,
            cy: self.height,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        // SAFETY: every pointer is to a local that outlives the call.
        unsafe {
            UpdateLayeredWindow(
                window,
                None,
                Some(&position),
                Some(&size),
                Some(self.dc),
                Some(&source),
                windows::Win32::Foundation::COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }

    /// The frame as a PNG, for checking what was drawn without a screen.
    pub fn to_png(&self) -> Option<Vec<u8>> {
        let count = self.width as usize * self.height as usize * 4;
        // SAFETY: the section is `count` bytes for as long as `self` lives.
        let pixels = unsafe { std::slice::from_raw_parts(self.pixels, count) };
        let mut rgba = Vec::with_capacity(count);
        for pixel in pixels.as_chunks::<4>().0 {
            let alpha = u32::from(pixel[3]);
            let straight = |channel: u8| -> u8 {
                (u32::from(channel) * 255 + alpha / 2)
                    .checked_div(alpha)
                    .unwrap_or(0)
                    .min(255) as u8
            };
            rgba.extend_from_slice(&[
                straight(pixel[2]),
                straight(pixel[1]),
                straight(pixel[0]),
                pixel[3],
            ]);
        }
        let mut png = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png);
        image::ImageEncoder::write_image(
            encoder,
            &rgba,
            self.width as u32,
            self.height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
        Some(png)
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: selects the old bitmap back before deleting what `new` made.
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

#[derive(Hash, PartialEq, Eq)]
struct FormatKey {
    size_tenths: u32,
    bold: bool,
    mono: bool,
    align: u8,
    wrap: bool,
}

pub struct Painter {
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    wic: IWICImagingFactory,
    target: ID2D1DCRenderTarget,
    formats: HashMap<FormatKey, IDWriteTextFormat>,
    icons: Vec<(Icon, Vec<ID2D1PathGeometry>)>,
    bitmaps: HashMap<String, ID2D1Bitmap>,
}

fn color_f(color: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: f32::from(color.a) / 255.0,
    }
}

fn rect_f(rect: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: rect.x,
        top: rect.y,
        right: rect.x + rect.w,
        bottom: rect.y + rect.h,
    }
}

impl Painter {
    /// Needs COM initialised on this thread.
    pub fn new() -> Result<Self> {
        // SAFETY: plain factory creation.
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                // Scaled by hand with a transform, so all of it is in logical pixels.
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = factory.CreateDCRenderTarget(&properties)?;
            // Text on transparency cannot use ClearType.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(Self {
                factory,
                dwrite,
                wic,
                target,
                formats: HashMap::new(),
                icons: Vec::new(),
                bitmaps: HashMap::new(),
            })
        }
    }

    fn format(&mut self, style: &TextStyle) -> Result<IDWriteTextFormat> {
        let key = FormatKey {
            size_tenths: (style.size * 10.0) as u32,
            bold: style.bold,
            mono: style.mono,
            align: style.align as u8,
            wrap: style.wrap,
        };
        if let Some(format) = self.formats.get(&key) {
            return Ok(format.clone());
        }
        // SAFETY: strings are constants; the format is reference counted.
        unsafe {
            let family = if style.mono {
                w!("Consolas")
            } else {
                w!("Segoe UI")
            };
            let weight = if style.bold {
                DWRITE_FONT_WEIGHT_SEMI_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            };
            let format = self.dwrite.CreateTextFormat(
                family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                style.size,
                w!("en-us"),
            )?;
            format.SetTextAlignment(match style.align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            })?;
            if style.wrap {
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
                format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            } else {
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            }
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            let sign = self.dwrite.CreateEllipsisTrimmingSign(&format)?;
            format.SetTrimming(&trimming, &sign)?;
            self.formats.insert(key, format.clone());
            Ok(format)
        }
    }

    fn icon(&mut self, icon: Icon) -> Result<Vec<ID2D1PathGeometry>> {
        if let Some((_, shapes)) = self.icons.iter().find(|(known, _)| *known == icon) {
            return Ok(shapes.clone());
        }
        let mut shapes = Vec::new();
        for polygon in polygons(icon) {
            // SAFETY: the sink is closed before the geometry is used.
            unsafe {
                let geometry = self.factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
                let points: Vec<Vector2> = polygon
                    .iter()
                    .map(|&(x, y)| Vector2 { X: x, Y: y })
                    .collect();
                sink.BeginFigure(points[0], D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLines(&points[1..]);
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
                shapes.push(geometry);
            }
        }
        self.icons.push((icon, shapes.clone()));
        Ok(shapes)
    }

    /// The image under `key`, decoded the first time it is drawn.
    fn bitmap(&mut self, key: &str, images: &HashMap<String, Arc<Vec<u8>>>) -> Option<ID2D1Bitmap> {
        if let Some(bitmap) = self.bitmaps.get(key) {
            return Some(bitmap.clone());
        }
        let bytes = images.get(key)?;
        // SAFETY: the stream reads `bytes` only during these calls.
        let bitmap = unsafe {
            (|| -> Result<ID2D1Bitmap> {
                let stream = self.wic.CreateStream()?;
                stream.InitializeFromMemory(bytes)?;
                let decoder = self.wic.CreateDecoderFromStream(
                    &stream,
                    std::ptr::null(),
                    WICDecodeMetadataCacheOnDemand,
                )?;
                let frame = decoder.GetFrame(0)?;
                let converter = self.wic.CreateFormatConverter()?;
                converter.Initialize(
                    &frame,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeMedianCut,
                )?;
                self.target.CreateBitmapFromWicBitmap(&converter, None)
            })()
        };
        match bitmap {
            Ok(bitmap) => {
                self.bitmaps.insert(key.to_owned(), bitmap.clone());
                Some(bitmap)
            }
            // A damaged cover is not worth a broken frame: the rest still draws.
            Err(err) => {
                log::debug!("could not decode an image: {}", err.message());
                None
            }
        }
    }

    /// Forgets decoded images that are no longer offered.
    pub fn forget_images(&mut self, images: &HashMap<String, Arc<Vec<u8>>>) {
        self.bitmaps.retain(|key, _| images.contains_key(key));
    }

    /// Draws `scene` onto `surface`, at `scale` physical pixels to the logical one.
    pub fn render<A>(
        &mut self,
        scene: &Scene<A>,
        scale: f32,
        images: &HashMap<String, Arc<Vec<u8>>>,
        surface: &Surface,
    ) -> Result<()> {
        let target = self.target.clone();
        let bounds = RECT {
            left: 0,
            top: 0,
            right: surface.width,
            bottom: surface.height,
        };
        // SAFETY: drawing calls between BeginDraw and EndDraw on a target bound to a live DC.
        unsafe {
            target.BindDC(surface.dc, &bounds)?;
            target.BeginDraw();
            target.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
            let base = Matrix3x2::scale(scale, scale);
            target.SetTransform(&base);
            for op in &scene.ops {
                self.draw(&target, op, &base, images)?;
            }
            target.EndDraw(None, None)?;
        }
        Ok(())
    }

    /// Draws one op; the caller holds the target between BeginDraw and EndDraw.
    unsafe fn draw(
        &mut self,
        target: &ID2D1DCRenderTarget,
        op: &Op,
        base: &Matrix3x2,
        images: &HashMap<String, Arc<Vec<u8>>>,
    ) -> Result<()> {
        // SAFETY: all calls are on a target inside BeginDraw.
        unsafe {
            match op {
                Op::Fill {
                    rect,
                    radius,
                    color,
                } => {
                    let brush = target.CreateSolidColorBrush(&color_f(*color), None)?;
                    target.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: rect_f(*rect),
                            radiusX: *radius,
                            radiusY: *radius,
                        },
                        &brush,
                    );
                }
                Op::Island {
                    rect,
                    radius,
                    color,
                } => {
                    let brush = target.CreateSolidColorBrush(&color_f(*color), None)?;
                    // Rounded rectangle reaching above the top edge: only the bottom corners show.
                    let tall =
                        Rect::new(rect.x, rect.y - radius * 2.0, rect.w, rect.h + radius * 2.0);
                    target.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: rect_f(tall),
                            radiusX: *radius,
                            radiusY: *radius,
                        },
                        &brush,
                    );
                }
                Op::Outline {
                    rect,
                    radius,
                    color,
                    width,
                } => {
                    let brush = target.CreateSolidColorBrush(&color_f(*color), None)?;
                    target.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: rect_f(*rect),
                            radiusX: *radius,
                            radiusY: *radius,
                        },
                        &brush,
                        *width,
                        None,
                    );
                }
                Op::Text { rect, text, style } => {
                    if text.is_empty() {
                        return Ok(());
                    }
                    let format = self.format(style)?;
                    let brush = target.CreateSolidColorBrush(&color_f(style.color), None)?;
                    let wide: Vec<u16> = text.encode_utf16().collect();
                    target.DrawText(
                        &wide,
                        &format,
                        &rect_f(*rect),
                        &brush,
                        D2D1_DRAW_TEXT_OPTIONS_CLIP,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
                Op::Icon { rect, icon, color } => {
                    let shapes = self.icon(*icon)?;
                    let brush = target.CreateSolidColorBrush(&color_f(*color), None)?;
                    let scale = rect.w / 24.0;
                    let place = Matrix3x2::scale(scale, scale)
                        * Matrix3x2::translation(rect.x, rect.y)
                        * *base;
                    target.SetTransform(&place);
                    for shape in &shapes {
                        target.FillGeometry(shape, &brush, None);
                    }
                    target.SetTransform(base);
                }
                Op::Image { rect, radius, key } => {
                    let Some(bitmap) = self.bitmap(key, images) else {
                        let brush = target.CreateSolidColorBrush(
                            &color_f(keenu_ui::scene::palette::BUTTON),
                            None,
                        )?;
                        target.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: rect_f(*rect),
                                radiusX: *radius,
                                radiusY: *radius,
                            },
                            &brush,
                        );
                        return Ok(());
                    };
                    let size = bitmap.GetSize();
                    // Fills the shape, cut to fit.
                    let scale = (rect.w / size.width).max(rect.h / size.height);
                    let dx = rect.x + (rect.w - size.width * scale) / 2.0;
                    let dy = rect.y + (rect.h - size.height * scale) / 2.0;
                    let brush = target.CreateBitmapBrush(&bitmap, None, None)?;
                    brush.SetTransform(
                        &(Matrix3x2::scale(scale, scale) * Matrix3x2::translation(dx, dy)),
                    );
                    target.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: rect_f(*rect),
                            radiusX: *radius,
                            radiusY: *radius,
                        },
                        &brush,
                    );
                }
            }
        }
        Ok(())
    }
}
