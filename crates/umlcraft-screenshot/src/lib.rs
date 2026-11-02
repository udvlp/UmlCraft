//! A screenshot of the app's own window as the user sees it: title bar, ribbon, panels and
//! canvas, also while other windows cover it. Only Windows so far.
//!
//! Capture from a thread other than the window's: Windows asks the window's thread to draw
//! itself, which must be free to answer.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const UNSUPPORTED: &str = "Screenshots of the UmlCraft window are not supported on this system \
                           yet (only on Windows).";

/// The window to capture, taken from GPUI's window; unlike that, it can go to another thread.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    #[cfg_attr(not(windows), allow(dead_code))]
    hwnd: isize,
}

impl Target {
    pub fn of(window: &impl HasWindowHandle) -> Result<Self, String> {
        let handle = window.window_handle().map_err(|err| err.to_string())?;
        match handle.as_raw() {
            RawWindowHandle::Win32(handle) => Ok(Self { hwnd: handle.hwnd.get() }),
            _ => Err(UNSUPPORTED.into()),
        }
    }
}

/// Pixels row by row from the top, four bytes each: red, green, blue and alpha (opaque).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The window in physical pixels, without the invisible borders Windows adds for resizing.
pub fn capture(target: Target) -> Result<Image, String> {
    #[cfg(windows)]
    return win::capture(target.hwnd);
    #[cfg(not(windows))]
    {
        let _ = target;
        Err(UNSUPPORTED.into())
    }
}

/// A rectangle in pixels, right and bottom exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// The part `crop` of a top-down BGRA bitmap `width` pixels wide, as opaque RGBA; all of it
/// when `crop` leaves nothing.
#[cfg_attr(not(windows), allow(dead_code))]
fn crop_bgra(bgra: &[u8], width: i32, height: i32, crop: Rect) -> Image {
    let clamp = |v: i32, max: i32| v.clamp(0, max);
    let mut r = Rect {
        left: clamp(crop.left, width),
        top: clamp(crop.top, height),
        right: clamp(crop.right, width),
        bottom: clamp(crop.bottom, height),
    };
    if r.right <= r.left || r.bottom <= r.top {
        r = Rect { left: 0, top: 0, right: width, bottom: height };
    }
    let (w, h) = ((r.right - r.left) as usize, (r.bottom - r.top) as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in r.top..r.bottom {
        let row = (y * width + r.left) as usize * 4;
        for px in bgra[row..row + w * 4].as_chunks::<4>().0 {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    Image { width: w as u32, height: h as u32, rgba }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsIconic, IsWindow, PW_RENDERFULLCONTENT,
    };

    use super::{Image, Rect, crop_bgra};

    pub fn capture(hwnd: isize) -> Result<Image, String> {
        let hwnd = HWND(hwnd as *mut _);
        // SAFETY: plain Win32 calls on a window handle, which Windows checks itself.
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return Err("The UmlCraft window is gone.".into());
            }
            if IsIconic(hwnd).as_bool() {
                return Err("The UmlCraft window is minimized; ask the user to restore it.".into());
            }
            // Physical pixels whatever the thread was set to, like the frame bounds below.
            let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let image = grab(hwnd);
            if !previous.0.is_null() {
                SetThreadDpiAwarenessContext(previous);
            }
            image
        }
    }

    /// Draws the window into a bitmap: `PW_RENDERFULLCONTENT` takes the content as composed
    /// (GPUI draws with DirectX), also where other windows cover it.
    unsafe fn grab(hwnd: HWND) -> Result<Image, String> {
        // SAFETY: the bitmap and the device contexts are released on every path; the buffer
        // holds what GetDIBits writes for the size the header gives.
        unsafe {
            let mut window = RECT::default();
            GetWindowRect(hwnd, &mut window).map_err(|err| err.to_string())?;
            let mut visible = window;
            let bounds = DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                (&raw mut visible).cast(),
                size_of::<RECT>() as u32,
            );
            if bounds.is_err() {
                visible = window;
            }
            let (width, height) = (window.right - window.left, window.bottom - window.top);
            if width <= 0 || height <= 0 {
                return Err("The UmlCraft window has no size.".into());
            }

            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            let old = SelectObject(dc, bitmap.into());
            let printed = PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();
            SelectObject(dc, old);
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    // Negative: rows from the top.
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bgra = vec![0u8; width as usize * height as usize * 4];
            let rows = GetDIBits(
                dc,
                bitmap,
                0,
                height as u32,
                Some(bgra.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            );
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            if !printed || rows != height {
                return Err("Windows could not capture the UmlCraft window.".into());
            }
            let crop = Rect {
                left: visible.left - window.left,
                top: visible.top - window.top,
                right: visible.right - window.left,
                bottom: visible.bottom - window.top,
            };
            Ok(crop_bgra(&bgra, width, height, crop))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cropping_keeps_the_visible_part_as_opaque_rgba() {
        // 3 × 2 pixels, blue channel = index.
        let bgra: Vec<u8> = (0..6).flat_map(|i| [i, 10, 20, 0]).collect();
        let crop = Rect { left: 1, top: 1, right: 3, bottom: 2 };
        let image = crop_bgra(&bgra, 3, 2, crop);
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, vec![20, 10, 4, 255, 20, 10, 5, 255]);
        // A crop outside the bitmap keeps all of it.
        let none = Rect { left: 5, top: 5, right: 9, bottom: 9 };
        assert_eq!(crop_bgra(&bgra, 3, 2, none).width, 3);
    }
}
