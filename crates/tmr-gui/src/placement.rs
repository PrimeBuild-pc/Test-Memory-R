//! Where the window is on the screen (the owner, from the forum, 2026-11-23: it kept its size and not its place): its frame's top-left as
//! it closed, in screen pixels, and whether it was maximized, kept in `window.json` beside its size and put back as it opens, but only onto
//! a screen that is there now. Reactor 0.100.0 sizes a window (`WindowVisuals::client_size`: AppWindow's ResizeClient, before it shows
//! it) but can't place one, nor gives its handle (TODO 111), so this is Win32, on the window's own thread: the handle is the thread's WinUI
//! window, found as the first size arrives (the window is there then, and shown: it may show for a moment where Windows put it);
//! SetWindowPos moves it, as AppWindow's Move would; and a window procedure of ours in front of WinUI's notes each move, as the window is
//! gone by the time it is dropped and its settings saved. User32's own subclassing, not comctl32's SetWindowSubclass: tmr.exe imports no
//! comctl32, and would load it in every run, the console's too.
//!
//! Maximized is kept: closed maximized (or minimized from maximized), the window opens maximized on the screen it was on, as Windows' own
//! placement does, and the size kept is the one it had before, which it restores to. Minimized isn't: it opens as it would restore, since a
//! window opening minimized looks like one that didn't open. A placement that isn't mostly on a screen now (one unplugged, or its
//! resolution or scaling changed) is dropped whole: the window opens where Windows puts it, at its size, not maximized.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, EnumThreadWindows, GWLP_WNDPROC, GetClassNameW, GetWindowLongPtrW, GetWindowRect, IsIconic, IsZoomed,
    SW_MAXIMIZE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_NCDESTROY, WM_WINDOWPOSCHANGED,
    WNDPROC,
};
use windows::core::BOOL;

/// The class of a WinUI 3 window's own HWND, which Windows App SDK registers: TMR has one such window, on Reactor's thread
const WINUI_WINDOW_CLASS: &str = "WinUIDesktopWin32WindowClass";
/// How far below the window's top a point of its title bar is looked for on a screen: one there, the window can be dragged by it
const TITLE_BAR: i32 = 8;

/// Where the window was, as `window.json` keeps it
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Its frame's top-left, in screen pixels (Windows' virtual screen: the primary screen's top-left is 0, 0), where it last was neither
    /// maximized nor minimized
    pub position: (i32, i32),
    /// It was maximized, or minimized from maximized
    pub maximized: bool,
}

/// A rectangle in screen pixels, as Win32's RECT has it: `right` and `bottom` just outside it
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }

    /// Its area in square pixels, none when it is empty
    fn area(self) -> i64 {
        i64::from(self.width().max(0)) * i64::from(self.height().max(0))
    }

    /// The area it shares with `other`
    fn overlap(self, other: Rect) -> i64 {
        let shared = Rect {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        shared.area()
    }

    fn contains(self, (x, y): (i32, i32)) -> bool {
        (self.left..self.right).contains(&x) && (self.top..self.bottom).contains(&y)
    }
}

impl From<RECT> for Rect {
    fn from(rect: RECT) -> Self {
        Rect { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom }
    }
}

/// Whether `window` would be mostly on a screen, `screens` being the work areas there now (each screen less its taskbar): at least half of
/// it on one of them, and the middle of its title bar on one, to drag it by. An empty one is on none.
pub fn on_screen(window: Rect, screens: &[Rect]) -> bool {
    let area = window.area();
    let title_bar = (window.left.saturating_add(window.width() / 2), window.top.saturating_add(TITLE_BAR));
    area > 0
        && screens.iter().any(|screen| screen.overlap(window).saturating_mul(2) >= area)
        && screens.iter().any(|screen| screen.contains(title_bar))
}

thread_local! {
    /// The window, from `attach` until it is destroyed
    static WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
    /// The window's procedure before ours (`follow`), which takes every message after it
    static PREVIOUS: Cell<WNDPROC> = const { Cell::new(None) };
    /// `attach` has run: once a window
    static ATTACHED: Cell<bool> = const { Cell::new(false) };
    /// Where the window is, as its last move left it (`note`): none when it isn't followed
    static LAST: Cell<Option<Placement>> = const { Cell::new(None) };
}

/// The window found, put where `saved` says when that is on a screen now (maximized again if it was), and followed from then on (`last`):
/// once, as its first size arrives, on its thread. Not found (a later Windows App SDK might name its class otherwise), it stays where
/// Windows put it, and `window.json` keeps the placement it had.
pub fn attach(saved: Option<Placement>) {
    if ATTACHED.replace(true) {
        return;
    }
    let Some(window) = find() else { return };
    WINDOW.set(Some(window));
    // Where the saved position would put the window at the size Reactor gave it; none to restore when it would be off the screens, or
    // when something else has maximized or minimized it already
    let restore = saved.filter(|saved| {
        let (x, y) = saved.position;
        frame(window).is_some_and(|now| {
            let there = Rect { left: x, top: y, right: x.saturating_add(now.width()), bottom: y.saturating_add(now.height()) };
            restored() && on_screen(there, &screens())
        })
    });
    if let Some(restore) = restore {
        let (x, y) = restore.position;
        // SAFETY: the window is this thread's; the size and the z-order are left as they are
        let _ = unsafe { SetWindowPos(window, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
    }
    note(window);
    // The procedure ours passes each message on to, kept before ours takes the first. SAFETY: the window is this thread's; a window's
    // procedure, which GWLP_WNDPROC holds as an integer, is a WNDPROC, 0 being none
    PREVIOUS.set(unsafe { std::mem::transmute::<isize, WNDPROC>(GetWindowLongPtrW(window, GWLP_WNDPROC)) });
    // SAFETY: the window is this thread's; `follow` passes every message on to the procedure it replaces, until the window is destroyed
    if PREVIOUS.get().is_none() || unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, follow as *const () as isize) } == 0 {
        // Not followed, nothing is kept: `window.json` keeps what it had
        LAST.set(None);
        return;
    }
    // After the note: its restored position is the one moved to, and `follow` notes the maximize
    if restore.is_some_and(|restore| restore.maximized) {
        // SAFETY: the window is this thread's
        let _ = unsafe { ShowWindow(window, SW_MAXIMIZE) };
    }
}

/// Where the window was as its last move left it; none when it wasn't followed, and `window.json` keeps what it had
pub fn last() -> Option<Placement> {
    LAST.get()
}

/// The window is neither maximized nor minimized, or isn't known yet: the size it shows is the one to keep
pub fn restored() -> bool {
    // SAFETY: the window is this thread's, and `WINDOW` is cleared as it is destroyed
    WINDOW.get().is_none_or(|window| unsafe { !IsZoomed(window).as_bool() && !IsIconic(window).as_bool() })
}

/// The calling thread's WinUI window, if it has one
fn find() -> Option<HWND> {
    unsafe extern "system" fn each(window: HWND, found: LPARAM) -> BOOL {
        let mut class = [0u16; 64];
        // SAFETY: the enumeration's window; the buffer is this frame's, and the call writes within it
        let length = usize::try_from(unsafe { GetClassNameW(window, &mut class) }).unwrap_or(0);
        if !class[..length].iter().copied().eq(WINUI_WINDOW_CLASS.encode_utf16()) {
            return true.into();
        }
        // SAFETY: `found` is `find`'s `Option<HWND>`, which outlives the enumeration, and the enumeration calls this on `find`'s thread
        unsafe { *(found.0 as *mut Option<HWND>) = Some(window) };
        false.into()
    }
    let mut found: Option<HWND> = None;
    // SAFETY: `found` outlives the call, which calls `each` before it returns; FALSE from `each` only ends the enumeration
    let _ = unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(each), LPARAM(&mut found as *mut Option<HWND> as isize)) };
    found
}

/// The work areas of the screens there now (each screen less its taskbar), in screen pixels
fn screens() -> Vec<Rect> {
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, screens: LPARAM) -> BOOL {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY: the enumeration's monitor; `info` is this frame's, its size set
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            // SAFETY: `screens` is `screens`'s Vec, which outlives the enumeration, and the enumeration calls this on its thread
            unsafe { &mut *(screens.0 as *mut Vec<Rect>) }.push(info.rcWork.into());
        }
        true.into()
    }
    let mut screens: Vec<Rect> = Vec::new();
    // SAFETY: `screens` outlives the call, which calls `each` before it returns; every monitor, no clipping
    let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut screens as *mut Vec<Rect> as isize)) };
    screens
}

/// The window's frame, in screen pixels
fn frame(window: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    // SAFETY: the window is this thread's; `rect` is this frame's
    unsafe { GetWindowRect(window, &mut rect) }.ok()?;
    Some(rect.into())
}

/// Where the window is now, into `LAST`: its frame's top-left while it is neither maximized nor minimized; maximized, where it was before,
/// flagged; minimized, nothing new, so it keeps what the window restores to
fn note(window: HWND) {
    // SAFETY: the window is this thread's
    if unsafe { IsIconic(window) }.as_bool() {
        return;
    }
    // SAFETY: as above
    let maximized = unsafe { IsZoomed(window) }.as_bool();
    let position = match frame(window) {
        Some(frame) if !maximized => Some((frame.left, frame.top)),
        _ => LAST.get().map(|last| last.position),
    };
    LAST.set(position.map(|position| Placement { position, maximized }));
}

/// Our window procedure, in front of WinUI's: each move noted (WM_WINDOWPOSCHANGED, which a resize, a maximize, a minimize and a DPI
/// change send too), the window forgotten as it goes (WM_NCDESTROY, its last message), and every message passed on as it came
unsafe extern "system" fn follow(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_WINDOWPOSCHANGED => note(window),
        WM_NCDESTROY => WINDOW.set(None),
        _ => {}
    }
    match PREVIOUS.get() {
        // SAFETY: the window's procedure before ours, on its thread, with the message as it came
        Some(previous) => unsafe { CallWindowProcW(Some(previous), window, message, wparam, lparam) },
        // SAFETY: as above; not reached, as `attach` installs `follow` only with a procedure to pass on to
        None => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, width: i32, height: i32) -> Rect {
        Rect { left, top, right: left + width, bottom: top + height }
    }

    /// A window is kept where it was while at least half of it, and its title bar, is on a screen there now; else it goes where Windows
    /// puts it
    #[test]
    fn a_placement_is_kept_only_mostly_on_a_screen_there_now() {
        // One 1920 x 1080 screen, its taskbar at the bottom; a second, 2560 x 1440, to its left
        let primary = rect(0, 0, 1920, 1032);
        let left = rect(-2560, 0, 2560, 1392);
        let window = |left: i32, top: i32| rect(left, top, 900, 1000);
        // On the screen, at its edge, and across both screens with half on one
        assert!(on_screen(window(100, 20), &[primary]));
        assert!(on_screen(window(0, 0), &[primary]));
        assert!(on_screen(window(-450, 20), &[primary, left]));
        // The screen on the left unplugged; the primary at a lower resolution than it closed on
        assert!(!on_screen(window(-1500, 20), &[primary]));
        assert!(!on_screen(window(2000, 600), &[primary]));
        // Under half on any screen, though all of it is on some: corners of four screens meeting under it
        let quarters = [rect(-1000, -1000, 1000, 1000), rect(0, -1000, 1000, 1000), rect(-1000, 0, 1000, 1000), rect(0, 0, 1000, 1000)];
        assert!(!on_screen(rect(-300, -300, 600, 600), &quarters));
        // Mostly on, but its title bar above the screen, or behind a taskbar at the top
        assert!(!on_screen(window(100, -40), &[primary]));
        assert!(!on_screen(window(100, 0), &[rect(0, 48, 1920, 1032)]));
        // Just over half on, its title bar's middle 10 pixels in; no screens, or an empty window, is never on one
        assert!(on_screen(window(1920 - 460, 20), &[primary]));
        assert!(!on_screen(window(100, 20), &[]));
        assert!(!on_screen(rect(100, 20, 0, 0), &[primary]));
        // A position near the ends of i32 (a hand-edited window.json) saturates instead of overflowing
        assert!(!on_screen(rect(i32::MAX - 10, i32::MAX - 10, 10, 10), &[primary]));
    }
}
