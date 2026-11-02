//! A screenshot of the app's own window as the user sees it: title bar, ribbon, panels and
//! canvas, also while other windows cover it, while it is minimized or while it lies beyond the
//! screens; and the window's size and place for such screenshots. Only Windows so far.
//!
//! Call these from a thread other than the window's: Windows asks the window's thread to draw
//! itself and to take its new size, which must be free to answer.

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
/// A minimized window is captured as if restored; the user does not see it come back.
pub fn capture(target: Target) -> Result<Image, String> {
    #[cfg(windows)]
    return win::capture(target.hwnd);
    #[cfg(not(windows))]
    {
        let _ = target;
        Err(UNSUPPORTED.into())
    }
}

/// Where the window is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// On a screen, where the user sees it.
    Screen,
    /// Beyond the right edge of the screens: drawn as usual and captured, but nobody sees it.
    Offscreen,
    /// Minimized to the taskbar; captured as if restored.
    Minimized,
}

impl Placement {
    pub fn name(self) -> &'static str {
        match self {
            Placement::Screen => "screen",
            Placement::Offscreen => "offscreen",
            Placement::Minimized => "minimized",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Placement::Screen, Placement::Offscreen, Placement::Minimized]
            .into_iter()
            .find(|p| p.name() == name)
    }
}

/// The window's size as [`capture`] takes it (physical pixels) and its place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub placement: Placement,
    /// Physical pixels per logical pixel (2 at 200 %).
    pub scale: f32,
}

/// Gives the window the size `width` × `height` as [`capture`] takes it (each `None` keeps it)
/// and moves it to `placement` (`None` keeps it), without taking the focus from other windows.
pub fn arrange(
    target: Target,
    width: Option<u32>,
    height: Option<u32>,
    placement: Option<Placement>,
) -> Result<WindowState, String> {
    #[cfg(windows)]
    return win::arrange(target.hwnd, width, height, placement);
    #[cfg(not(windows))]
    {
        let _ = (target, width, height, placement);
        Err(UNSUPPORTED.into())
    }
}

/// The window's size and place.
pub fn state(target: Target) -> Result<WindowState, String> {
    arrange(target, None, None, None)
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
    use std::thread::sleep;
    use std::time::Duration;

    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{
        DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_TRANSITIONS_FORCEDISABLED, DwmGetWindowAttribute,
        DwmSetWindowAttribute,
    };
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetThreadDpiAwarenessContext,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowPlacement, GetWindowRect, IsIconic, IsWindow,
        PW_RENDERFULLCONTENT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN, SPI_GETWORKAREA, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
        SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetWindowPlacement, SetWindowPos,
        SystemParametersInfoW, WINDOWPLACEMENT,
    };
    use windows::core::BOOL;

    use super::{Image, Placement, Rect, WindowState, crop_bgra};

    /// How long a window restored from the taskbar takes to draw itself again.
    const REDRAW: Duration = Duration::from_millis(300);
    /// Room between the screens' right edge and a window placed beyond it.
    const BEYOND: i32 = 100;

    pub fn capture(hwnd: isize) -> Result<Image, String> {
        let hwnd = window(hwnd)?;
        physical(|| {
            let parked = park(hwnd);
            if parked.is_some() {
                sleep(REDRAW);
            }
            // SAFETY: see `grab`.
            let image = unsafe { grab(hwnd) };
            if let Some(parked) = parked {
                unpark(hwnd, parked);
            }
            image
        })
    }

    pub fn arrange(
        hwnd: isize,
        width: Option<u32>,
        height: Option<u32>,
        placement: Option<Placement>,
    ) -> Result<WindowState, String> {
        let hwnd = window(hwnd)?;
        physical(|| {
            let was = current(hwnd);
            let placement = placement.unwrap_or(was);
            let unchanged = width.is_none() && height.is_none() && placement == was;
            // A minimized window has no frame to measure; restore it out of sight first.
            let parked = park(hwnd);
            let (outer, visible) = bounds(hwnd);
            let size = |given: Option<u32>, now: i32, frame: i32| {
                given.map_or(now, |v| v.clamp(1, 16_000) as i32) + frame
            };
            let w = size(width, visible.right - visible.left, frame_width(outer, visible));
            let h = size(height, visible.bottom - visible.top, frame_height(outer, visible));
            if unchanged {
                if let Some(parked) = parked {
                    unpark(hwnd, parked);
                }
            } else {
                let on_screen = || match &parked {
                    Some(parked) if was != Placement::Offscreen => {
                        let r = to_screen(parked.placement.rcNormalPosition);
                        (r.left, r.top)
                    }
                    _ if was == Placement::Screen => (outer.left, outer.top),
                    _ => centred(w, h),
                };
                match placement {
                    Placement::Screen | Placement::Offscreen => {
                        let (x, y) =
                            if placement == Placement::Offscreen { beyond() } else { on_screen() };
                        // SAFETY: a plain Win32 call on a checked window handle.
                        unsafe {
                            let _ =
                                SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
                        }
                        if parked.is_some() {
                            transitions(hwnd, true);
                        }
                    }
                    Placement::Minimized => {
                        // To the taskbar straight away; restored, it comes back on a screen.
                        let (x, y) = on_screen();
                        let mut wp = parked.map_or_else(|| self::placement(hwnd), |p| p.placement);
                        wp.rcNormalPosition =
                            to_workspace(RECT { left: x, top: y, right: x + w, bottom: y + h });
                        wp.showCmd = SW_SHOWMINNOACTIVE.0 as u32;
                        transitions(hwnd, false);
                        // SAFETY: the placement is a full WINDOWPLACEMENT with its length set.
                        unsafe {
                            let _ = SetWindowPlacement(hwnd, &wp);
                        }
                        transitions(hwnd, true);
                    }
                }
            }
            Ok(WindowState {
                width: (w - frame_width(outer, visible)) as u32,
                height: (h - frame_height(outer, visible)) as u32,
                placement: current(hwnd),
                // SAFETY: a plain Win32 call on a checked window handle.
                scale: unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0,
            })
        })
    }

    fn window(hwnd: isize) -> Result<HWND, String> {
        let hwnd = HWND(hwnd as *mut _);
        // SAFETY: Windows checks the handle itself.
        if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            Ok(hwnd)
        } else {
            Err("The UmlCraft window is gone.".into())
        }
    }

    /// Runs `f` with this thread counting in physical pixels, like the frame bounds.
    fn physical<T>(f: impl FnOnce() -> T) -> T {
        // SAFETY: switches this thread's DPI awareness and back.
        unsafe {
            let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let out = f();
            if !previous.0.is_null() {
                SetThreadDpiAwarenessContext(previous);
            }
            out
        }
    }

    fn current(hwnd: HWND) -> Placement {
        // SAFETY: a plain Win32 call on a checked window handle.
        if unsafe { IsIconic(hwnd) }.as_bool() {
            return Placement::Minimized;
        }
        let (_, visible) = bounds(hwnd);
        let screens = screens();
        let overlaps = visible.left < screens.right
            && visible.right > screens.left
            && visible.top < screens.bottom
            && visible.bottom > screens.top;
        if overlaps { Placement::Screen } else { Placement::Offscreen }
    }

    /// The window with its invisible resize borders, and the part one sees.
    fn bounds(hwnd: HWND) -> (RECT, RECT) {
        let mut outer = RECT::default();
        // SAFETY: both calls write one RECT, whose size is given.
        unsafe {
            let _ = GetWindowRect(hwnd, &mut outer);
            let mut visible = outer;
            let got = DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                (&raw mut visible).cast(),
                size_of::<RECT>() as u32,
            );
            (outer, if got.is_ok() { visible } else { outer })
        }
    }

    fn frame_width(outer: RECT, visible: RECT) -> i32 {
        (outer.right - outer.left) - (visible.right - visible.left)
    }

    fn frame_height(outer: RECT, visible: RECT) -> i32 {
        (outer.bottom - outer.top) - (visible.bottom - visible.top)
    }

    /// All screens together.
    fn screens() -> RECT {
        // SAFETY: plain Win32 calls.
        unsafe {
            let (left, top) =
                (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
            RECT {
                left,
                top,
                right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
                bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
            }
        }
    }

    /// Where a window goes that nobody should see.
    fn beyond() -> (i32, i32) {
        let screens = screens();
        (screens.right + BEYOND, screens.top)
    }

    /// The main screen without the taskbar.
    fn work_area() -> RECT {
        let mut area = RECT::default();
        // SAFETY: SPI_GETWORKAREA writes one RECT.
        unsafe {
            let _ = SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some((&raw mut area).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
        }
        area
    }

    /// The top left of a `w` × `h` window in the middle of the main screen.
    fn centred(w: i32, h: i32) -> (i32, i32) {
        let area = work_area();
        let x = area.left + ((area.right - area.left - w) / 2).max(0);
        let y = area.top + ((area.bottom - area.top - h) / 2).max(0);
        (x, y)
    }

    // A window's normal position counts from the main screen's work area, not the screen.
    fn to_screen(r: RECT) -> RECT {
        let area = work_area();
        RECT {
            left: r.left + area.left,
            top: r.top + area.top,
            right: r.right + area.left,
            bottom: r.bottom + area.top,
        }
    }

    fn to_workspace(r: RECT) -> RECT {
        let area = work_area();
        RECT {
            left: r.left - area.left,
            top: r.top - area.top,
            right: r.right - area.left,
            bottom: r.bottom - area.top,
        }
    }

    fn placement(hwnd: HWND) -> WINDOWPLACEMENT {
        let mut wp =
            WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        // SAFETY: the placement has its length set.
        unsafe {
            let _ = GetWindowPlacement(hwnd, &mut wp);
        }
        wp
    }

    /// Without the animation of minimizing and restoring (`false`), or with it again.
    fn transitions(hwnd: HWND, on: bool) {
        let disabled = BOOL::from(!on);
        // SAFETY: the attribute takes one BOOL, whose size is given.
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                (&raw const disabled).cast(),
                size_of::<BOOL>() as u32,
            );
        }
    }

    /// A minimized window, restored beyond the screens for a moment.
    struct Parked {
        placement: WINDOWPLACEMENT,
    }

    /// Restores a minimized window beyond the screens, where it draws itself without being
    /// seen or taking the focus; `None` for a window that is not minimized.
    fn park(hwnd: HWND) -> Option<Parked> {
        // SAFETY: a plain Win32 call on a checked window handle.
        if !unsafe { IsIconic(hwnd) }.as_bool() {
            return None;
        }
        let saved = placement(hwnd);
        let normal = saved.rcNormalPosition;
        let (x, y) = beyond();
        let mut wp = saved;
        wp.rcNormalPosition = to_workspace(RECT {
            left: x,
            top: y,
            right: x + normal.right - normal.left,
            bottom: y + normal.bottom - normal.top,
        });
        wp.showCmd = SW_SHOWNOACTIVATE.0 as u32;
        transitions(hwnd, false);
        // SAFETY: the placement is a full WINDOWPLACEMENT with its length set.
        unsafe {
            let _ = SetWindowPlacement(hwnd, &wp);
        }
        Some(Parked { placement: saved })
    }

    /// Minimizes a parked window again, with its normal position as before.
    fn unpark(hwnd: HWND, parked: Parked) {
        let mut wp = parked.placement;
        wp.showCmd = SW_SHOWMINNOACTIVE.0 as u32;
        // SAFETY: the placement is a full WINDOWPLACEMENT with its length set.
        unsafe {
            let _ = SetWindowPlacement(hwnd, &wp);
        }
        transitions(hwnd, true);
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
