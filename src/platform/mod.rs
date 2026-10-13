// Platform adapters — only where Slint/Windows forces us to (M8).
// Policy (ADR-0002): never implement TSF/IME ourselves.

pub mod dib;
pub mod hotkeys;
pub mod monitors;
pub mod quit;
pub mod tray;

use std::path::Path;

/// Copy `text` to the system clipboard, as CF_UNICODETEXT via the same FFI
/// `read_clipboard` uses (ADR-0025's write half: `clip.exe`'s OEM-codepage
/// stdin garbles non-ASCII, and "Copy page as Markdown" must carry CJK).
/// No clipboard crate — user32/kernel32 are already linked for winit.
/// Other targets report failure instead of pretending.
pub fn copy_to_clipboard(text: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        const CF_UNICODETEXT: u32 = 13;
        const GMEM_MOVEABLE: u32 = 0x0002;

        #[link(name = "user32")]
        extern "system" {
            fn OpenClipboard(hwnd: isize) -> i32;
            fn CloseClipboard() -> i32;
            fn EmptyClipboard() -> i32;
            fn SetClipboardData(format: u32, hmem: isize) -> isize;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
            fn GlobalLock(hmem: isize) -> *mut u16;
            fn GlobalUnlock(hmem: isize) -> i32;
            fn GlobalFree(hmem: isize) -> isize;
        }

        // UTF-16 units plus the terminating nul; the allocator wants bytes
        let mut units: Vec<u16> = text.encode_utf16().collect();
        units.push(0);
        let bytes = units.len() * std::mem::size_of::<u16>();

        unsafe {
            // another process may hold the clipboard open: a short retry
            // beats failing a copy over a transient lock
            let mut opened = false;
            for _ in 0..5 {
                if OpenClipboard(0) != 0 {
                    opened = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            if !opened {
                return false;
            }
            let ok = (|| {
                if EmptyClipboard() == 0 {
                    return false;
                }
                let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
                if handle == 0 {
                    return false;
                }
                let ptr = GlobalLock(handle);
                if ptr.is_null() {
                    GlobalFree(handle);
                    return false;
                }
                std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
                GlobalUnlock(handle);
                // success transfers ownership: the clipboard frees the block
                SetClipboardData(CF_UNICODETEXT, handle) != 0
            })();
            CloseClipboard();
            ok
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = text;
        false
    }
}

/// Say something to the user when there is no window to say it in.
///
/// The single-instance claim (ADR-0136) is the caller: it runs before
/// `AppWindow::new`, so a second launch that is going to be told to go away has
/// no tray icon, no toast band and no Slint popup to carry the message — a
/// message printed to a console the release build does not even have would be a
/// rule the user never learns about.
///
/// `MessageBoxW` rather than a Slint popup for the same reason the clipboard
/// above is hand-declared FFI: this runs before the Slint backend exists, and
/// `MessageBoxW` is one call with no event loop of its own. `MB_OK` only, so the
/// box is a statement and not a question the app has no handler for.
pub fn notify(title: &str, body: &str) {
    #[cfg(target_os = "windows")]
    {
        const MB_OK: u32 = 0x0000_0000;
        const MB_ICONINFORMATION: u32 = 0x0000_0040;

        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
        }

        fn wide(s: &str) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            std::ffi::OsStr::new(s)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }

        let (text, caption) = (wide(body), wide(title));
        // A message box that cannot be shown (a session 0 service, a headless
        // verify run) returns 0 rather than blocking; there is no second channel
        // here to fall back to, and a failed notice is not worth failing a
        // launch over.
        unsafe {
            let _ = MessageBoxW(0, text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONINFORMATION);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("quire: {title}: {body}");
    }
}

/// Open a folder in the system file manager (Windows: `explorer.exe <dir>`)
/// — the settings storage row's "Open folder" affordance. Only the launch
/// is checked; explorer returns odd exit codes by design.
pub fn open_folder(path: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .is_ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        false
    }
}

/// Open a file with whatever the system has registered for its type — an
/// attachment block's "Open" (SPEC §三十七 批次 A). `ShellExecuteW` rather than
/// `explorer.exe <path>`: explorer exits 0x1 on success by design, so the
/// subprocess can't tell a launched app from a blocked extension, while the
/// FFI call answers `> 32` only for a real launch. One call, so no `windows`
/// crate — the same rule that keeps user32 hand-declared above.
pub fn open_with_default(path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        const SW_SHOWDEFAULT: i32 = 10;

        #[link(name = "shell32")]
        extern "system" {
            fn ShellExecuteW(
                hwnd: isize,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show: i32,
            ) -> isize;
        }

        fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            s.encode_wide().chain(std::iter::once(0)).collect()
        }

        let verb = wide(std::ffi::OsStr::new("open"));
        let target = wide(path.as_os_str());
        // Below 33 the return value is an SE_ERR_* code, not an HINSTANCE.
        unsafe {
            ShellExecuteW(
                0,
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWDEFAULT,
            ) > 32
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        false
    }
}

/// Read the system clipboard as text (the rich-paste path, SPEC §二十七).
/// Direct Win32 FFI: a `Get-Clipboard` subprocess was measured at 7-10 s on
/// the dev desktop (PowerShell startup under AV), which no paste can wait
/// for, while `OpenClipboard`/`GetClipboardData` are microseconds and the
/// MSVC toolchain already links user32/kernel32 for winit — so no clipboard
/// crate is pulled in (dependency policy in DECISIONS). Reads
/// CF_UNICODETEXT only; other targets report absence instead of pretending.
pub fn read_clipboard() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        const CF_UNICODETEXT: u32 = 13;

        #[link(name = "user32")]
        extern "system" {
            fn IsClipboardFormatAvailable(format: u32) -> i32;
            fn OpenClipboard(hwnd: isize) -> i32;
            fn CloseClipboard() -> i32;
            fn GetClipboardData(format: u32) -> isize;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalLock(hmem: isize) -> *mut u16;
            fn GlobalUnlock(hmem: isize) -> i32;
        }

        unsafe {
            if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
                return None;
            }
            // another process may hold the clipboard open: a short retry
            // beats failing a paste over a transient lock
            let mut opened = false;
            for _ in 0..5 {
                if OpenClipboard(0) != 0 {
                    opened = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            if !opened {
                return None;
            }
            let text = (|| {
                let handle = GetClipboardData(CF_UNICODETEXT);
                if handle == 0 {
                    return None;
                }
                let ptr = GlobalLock(handle);
                if ptr.is_null() {
                    return None;
                }
                let mut len = 0usize;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                GlobalUnlock(handle);
                Some(s)
            })();
            CloseClipboard();
            let text = text?.trim_end_matches(['\r', '\n']).to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Read the clipboard as a picture, returning PNG bytes (SPEC §三十七 批次 A's
/// last open item: paste a screenshot). CF_DIBV5 first because it is the format
/// that carries a real alpha channel, CF_DIB as the fallback every app writes.
/// Same rule as `read_clipboard`: hand-declared FFI, no clipboard crate, and the
/// decode itself lives in `dib` so it can be tested without a clipboard.
pub fn read_clipboard_image() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    {
        const CF_DIB: u32 = 8;
        const CF_DIBV5: u32 = 17;

        #[link(name = "user32")]
        extern "system" {
            fn IsClipboardFormatAvailable(format: u32) -> i32;
            fn OpenClipboard(hwnd: isize) -> i32;
            fn CloseClipboard() -> i32;
            fn GetClipboardData(format: u32) -> isize;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalSize(hmem: isize) -> usize;
            // the same declaration `read_clipboard` makes — one symbol declared
            // twice with different types is a warning, so the cast happens here
            fn GlobalLock(hmem: isize) -> *mut u16;
            fn GlobalUnlock(hmem: isize) -> i32;
        }

        let format = [CF_DIBV5, CF_DIB]
            .into_iter()
            .find(|f| unsafe { IsClipboardFormatAvailable(*f) != 0 })?;
        unsafe {
            let mut opened = false;
            for _ in 0..5 {
                if OpenClipboard(0) != 0 {
                    opened = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            if !opened {
                return None;
            }
            let raw = (|| {
                let handle = GetClipboardData(format);
                if handle == 0 {
                    return None;
                }
                let size = GlobalSize(handle);
                let ptr = GlobalLock(handle).cast::<u8>();
                if ptr.is_null() || size == 0 {
                    return None;
                }
                let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
                GlobalUnlock(handle);
                Some(bytes)
            })();
            CloseClipboard();
            dib::dib_to_png(&raw?)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// `WS_EX_TOOLWINDOW` — the bit that keeps a window off the taskbar and out of
/// Alt+Tab. Spelled out because a caller has to read it back to prove it landed.
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;

/// `WS_EX_WINDOWEDGE` — the 1px pale ring Windows 10/11 draws around a top-level
/// window. Spelled out for the same reason as `WS_EX_TOOLWINDOW`: a caller reads
/// it back to prove it landed.
pub const WS_EX_WINDOWEDGE: u32 = 0x0000_0100;

/// The two extended-style questions a Slint window cannot answer about itself.
/// They live here together because both need the same HWND, and the HWND is the
/// only place the shell keeps them.
#[cfg(target_os = "windows")]
mod win_style {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: isize = super::WS_EX_TOOLWINDOW as isize;
    const WS_EX_WINDOWEDGE: isize = super::WS_EX_WINDOWEDGE as isize;

    // DWM's own two answers, which are the ones that actually work on Windows 11.
    // `dwmapi.dll` is part of the OS since Vista, so there is nothing to ship.
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: isize,
            attribute: u32,
            value: *const std::ffi::c_void,
            size: u32,
        ) -> i32;
        fn DwmGetWindowAttribute(
            hwnd: isize,
            attribute: u32,
            value: *mut std::ffi::c_void,
            size: u32,
        ) -> i32;
    }

    /// `DWMWA_BORDER_COLOR` — the colour DWM paints around the window.
    const DWMWA_BORDER_COLOR: u32 = 34;
    /// `DWMWA_WINDOW_CORNER_PREFERENCE`.
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    /// `DWMWA_COLOR_NONE` (0xFFFFFFFE): "no border, not the default one".
    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    /// `DWMWCP_DONOTROUND` (1): square corners. The card already asks for them
    /// with `radius: 0px`, but rounding lives here and only here.
    const DWMWCP_DONOTROUND: i32 = 1;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;

    // `GetWindowLongPtrW` is the 64-bit spelling, which is what this shell ships.
    #[link(name = "user32")]
    extern "system" {
        fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
        fn SetWindowPos(
            hwnd: isize,
            insert_after: isize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
    }

    /// The HWND behind a Slint window. It is only real once the window has been
    /// shown *and* the event loop has run a frame, so an answer asked on the same
    /// frame as `show()` is the expected failure rather than a bug.
    fn hwnd_of(window: &slint::Window) -> Result<isize, String> {
        // `handle` outlives `raw`: the borrowed window handle points into it
        let handle = window.window_handle();
        let raw = handle
            .window_handle()
            .map_err(|e| format!("no window handle: {e}"))?;
        let RawWindowHandle::Win32(win32) = raw.as_ref() else {
            return Err("the window is not a Win32 HWND".to_string());
        };
        Ok(win32.hwnd.get())
    }

    pub fn ex_style(window: &slint::Window) -> Option<u32> {
        let hwnd = hwnd_of(window).ok()?;
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
        // zero is `GetWindowLongPtrW`'s "not a window" answer as well as a legal
        // style, and no live top-level is ever that
        (style != 0).then(|| style as u32)
    }

    pub fn set_tool_window(window: &slint::Window) -> Result<(), String> {
        let hwnd = hwnd_of(window)?;
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if style == 0 {
                return Err("GetWindowLongPtrW failed".to_string());
            }
            if style & WS_EX_TOOLWINDOW != 0 {
                return Ok(());
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_TOOLWINDOW);
            // The shell latches the style when it builds the button, so writing
            // the bit alone would leave it there: the frame change is what makes
            // it re-read.
            let ok = SetWindowPos(
                hwnd,
                0,
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            if ok == 0 {
                return Err("SetWindowPos failed".to_string());
            }
        }
        Ok(())
    }

    /// Take down the pale ring Windows draws around the 速记 window (ADR-0153).
    ///
    /// **The Win32 style bit is not the answer, and this function is the record
    /// of why.** `WS_EX_WINDOWEDGE` is documented as "the window has a border
    /// with a raised edge" and winit seeds it on every window, clearing it only
    /// for **child** ones (`winit-0.30.13/.../window_state.rs:259` and `:288`);
    /// a frameless top-level keeps it whatever `no-frame` says. The obvious fix
    /// is therefore to clear that bit — and it does not work. Measured on the
    /// live window, with the write and the read-back side by side:
    ///
    /// ```text
    /// style=0x40198 want=0x40098 prev=0x40198 after=0x40198 setpos=1
    /// ```
    ///
    /// `SetWindowLongPtrW` returned the old style, took the new one, reported
    /// success, and `SetWindowPos` returned 1 — and the bit was still set when
    /// read straight back. Windows 11 paints that ring from **DWM**, which keeps
    /// its own copy; the style bit only ever described it.
    ///
    /// So the ring is asked for in DWM's own terms: `DWMWA_BORDER_COLOR` set to
    /// `DWMWA_COLOR_NONE`, and `DWMWA_WINDOW_CORNER_PREFERENCE` set to
    /// `DWMWCP_DONOTROUND` because rounding is decided here too and
    /// `radius: 0px` in `.slint` cannot reach it. Both are Windows 10 1607+ /
    /// 11 only, and both are best-effort by contract — an older build answers
    /// `E_INVALIDARG` and the window keeps a ring, which is a cosmetic miss on
    /// a composer that otherwise works.
    ///
    /// The style bit is still cleared afterwards, and the order is deliberate:
    /// the clear is cheap, and on a build where DWM honours the attribute the
    /// bit is not what the user sees anyway. Clearing it last means that if a
    /// future Windows *does* respect it, the two agree.
    pub fn clear_window_edge(window: &slint::Window) -> Result<(), String> {
        let hwnd = hwnd_of(window)?;
        let mut failed: Vec<String> = Vec::new();

        unsafe {
            let border = DWMWA_COLOR_NONE;
            if DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &border as *const u32 as *const std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            ) != 0
            {
                failed.push("DWMWA_BORDER_COLOR".to_string());
            }
            let corner = DWMWCP_DONOTROUND;
            if DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const i32 as *const std::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            ) != 0
            {
                failed.push("DWMWA_WINDOW_CORNER_PREFERENCE".to_string());
            }

            // The bit itself, best-effort, for the reason in the doc comment.
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if style != 0 && style & WS_EX_WINDOWEDGE != 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !WS_EX_WINDOWEDGE);
            }
            // The shell draws the frame from a latched copy, so the attribute
            // write alone is not what the user sees.
            SetWindowPos(
                hwnd,
                0,
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(format!("DWM declined {}", failed.join(" and ")))
        }
    }

    /// `Option`, so 「DWM says no border」 and 「DWM would not answer」 stay
    /// different: the first is a pass, the second is a question this platform
    /// cannot answer and must not be reported as a failure.
    pub fn dwm_border_suppressed(window: &slint::Window) -> Option<bool> {
        let hwnd = hwnd_of(window).ok()?;
        let mut color: u32 = 0;
        let ok = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &mut color as *mut u32 as *mut std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            )
        };
        if ok != 0 {
            return None;
        }
        Some(color == DWMWA_COLOR_NONE)
    }
}

/// Take a pop-up out of the taskbar and out of Alt+Tab — the thing a 速记 window
/// owes the user, since a composer that vanishes on Escape has no business
/// keeping a button on the bar (ADR-0145).
///
/// Slint has no property for it: its window flags stop at `always-on-top` and
/// `no-frame`, so the answer is the shell's own extended style. Best-effort by
/// contract — the caller logs, and the note still gets written.
pub fn hide_from_taskbar(window: &slint::Window) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        win_style::set_tool_window(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        Err("this platform has no taskbar to leave out".to_string())
    }
}

/// Take down the pale ring Windows draws around the 速记 window (ADR-0153).
///
/// The companion to `hide_from_taskbar`, and needed for the same underlying
/// reason: `no-frame: true` is not the whole answer on a top-level window. winit
/// seeds `WS_EX_WINDOWEDGE` on every window and only clears it for **child**
/// windows, so a frameless top-level keeps the ring. The card's own `outlined:
/// false` (ADR-0152) removed a *different* line — the `border-strong` it drew
/// itself — and could not have reached this one.
///
/// Best-effort by contract, like its neighbour: the caller logs, and the window
/// is still a working composer without the ring.
pub fn clear_window_edge(window: &slint::Window) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        win_style::clear_window_edge(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        Err("this platform draws no window edge to take down".to_string())
    }
}

/// Whether DWM is actually suppressing the border on this window (ADR-0153).
///
/// The read-back half of `clear_window_edge`, and the only assertion worth
/// making about the ring. A `SetWindowLongPtrW` that returns success is worth
/// nothing here — measured, it returns the old style and leaves the bit set — so
/// the question is put to DWM, which is the thing that paints.
///
/// `None` when the attribute cannot be read at all (a pre-1607 build, or a window
/// whose HWND is not up yet). The caller reads that as "cannot tell" rather than
/// as "still there": a false accusation would fail the gate on a platform where
/// the question does not apply.
pub fn dwm_border_suppressed(window: &slint::Window) -> Option<bool> {
    #[cfg(target_os = "windows")]
    {
        win_style::dwm_border_suppressed(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        Some(true)
    }
}

/// The window's `GWL_EXSTYLE` bits as the shell actually holds them.
///
/// A caller cannot prove a style landed from the write alone, and `--quick-note`
/// (ADR-0145) is the only one that asks: it reads the bit back off the live
/// window instead of trusting that `hide_from_taskbar` returned `Ok`.
pub fn window_ex_style(window: &slint::Window) -> Option<u32> {
    #[cfg(target_os = "windows")]
    {
        win_style::ex_style(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_write_and_read_round_trip_unicode() {
        // the write path is FFI now (ADR-0025's second half): CJK survives
        let sample = "中文标题\n\n- item **bold**\nquire://page/7";
        assert!(copy_to_clipboard(sample), "the FFI write must succeed");
        let read = read_clipboard().expect("the FFI read must succeed");
        assert_eq!(read, sample, "the UTF-16 round trip preserves the text");
    }

    /// The one check the decoder cannot make on its own: that the FFI reads a
    /// picture some *other* process put on the clipboard. Ignored because it
    /// reads the user's real clipboard rather than a fixture -- run it by hand
    /// after loading a PNG (`Set-Clipboard` cannot do this; `[Windows.Forms.Clipboard]::SetImage`
    /// can, from an STA session).
    #[test]
    #[ignore = "reads the user's real clipboard"]
    fn a_picture_another_process_put_on_the_clipboard_decodes() {
        let png = read_clipboard_image().expect("the clipboard holds no CF_DIB / CF_DIBV5");
        let img = image::load_from_memory(&png).expect("the bytes must be a PNG");
        println!(
            "clipboard picture: {}x{} from {} PNG bytes",
            img.width(),
            img.height(),
            png.len()
        );
        assert!(img.width() > 0 && img.height() > 0);
    }
}
