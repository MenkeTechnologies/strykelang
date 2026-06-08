//! GUI automation builtins — full PyAutoGUI-equivalent surface
//! backed by the `enigo` crate (mouse + keyboard) and the `xcap`
//! crate (pixel + screenshot). Cross-platform: macOS CGEvent +
//! ScreenCaptureKit, X11 XTest + XGetImage, Wayland libei + portal,
//! Win32 SendInput + GDI. No external binaries; everything is in-
//! process per [[feedback_no_external_wrappers]].
//!
//! ──────────────────────────────────────────────────────────────────
//! Surface (all dispatched from `builtins.rs::try_builtin`):
//!
//! Mouse position / size
//!   mouse_pos()                  → [x, y]
//!   mouse_size() / screen_size() → [w, h]   (primary display)
//!   on_screen(x, y)              → 1 if `(x,y)` is on a display, else 0
//!
//! Mouse motion
//!   mouse_move(x, y, duration=0)              absolute, optional tween
//!   mouse_move_rel(dx, dy, duration=0)        relative, optional tween
//!   mouse_drag(x, y, duration=0, button=left)
//!   mouse_drag_rel(dx, dy, duration=0, button=left)
//!
//! Mouse buttons
//!   mouse_click(x?, y?, clicks=1, interval=0, button=left)
//!   mouse_right_click(x?, y?)
//!   mouse_middle_click(x?, y?)
//!   mouse_double_click(x?, y?, button=left)
//!   mouse_triple_click(x?, y?, button=left)
//!   mouse_down(button=left)
//!   mouse_up(button=left)
//!
//! Mouse wheel
//!   mouse_scroll(clicks, x?, y?)              vertical (positive = up)
//!   mouse_vscroll(clicks, x?, y?)             alias for mouse_scroll
//!   mouse_hscroll(clicks, x?, y?)             horizontal
//!
//! Keyboard
//!   key_type(text, interval=0)                type a literal string
//!   key_press(name, presses=1, interval=0)    discrete key press
//!   key_down(name)                            press without release
//!   key_up(name)                              release
//!   key_hotkey(@keys, interval=0)             chord: press in order,
//!                                             release in reverse
//!   keyboard_keys()                           → list of every
//!                                             recognized key name
//!
//! Screen / pixel
//!   pixel(x, y)                               → [r, g, b]
//!   pixel_matches_color(x, y, [r,g,b], tol=0) → 1 / 0
//!   screenshot(path?)                         capture primary display;
//!                                             if `path` given, writes
//!                                             PNG and returns the path;
//!                                             else returns the raw
//!                                             [w, h, RGBA-bytes] triple.
//!   screenshot_region(L, T, W, H, path?)      cropped variant
//!
//! ──────────────────────────────────────────────────────────────────
//! Returns: integer 0 on success for void ops; array `[a, b, ...]` for
//! queries. Errors propagate via `StrykeError::runtime` — macOS
//! Accessibility + Screen Recording denials, Wayland-portal failures,
//! and unknown key names all land there.
//!
//! Platform notes:
//! - macOS: first mouse/keyboard call prompts for Accessibility access
//!   for the terminal app launching `s`. The first pixel / screenshot
//!   call prompts for Screen Recording access separately. Both are
//!   one-time grants.
//! - X11 (Linux): no permission gates.
//! - Wayland (Linux): requires `wlroots-virtual-pointer` (mouse/kb)
//!   and the `org.freedesktop.portal.Screenshot` portal (pixel/screen).
//! - Windows: no permission gates.

use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;

use enigo::{
    Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings,
};
use std::thread::sleep as thread_sleep;
use std::time::Duration;

fn make_enigo() -> StrykeResult<Enigo> {
    Enigo::new(&Settings::default())
        .map_err(|e| StrykeError::runtime(format!("GUI init failed: {e}"), 0))
}

fn err<E: std::fmt::Display>(e: E, what: &str) -> StrykeError {
    StrykeError::runtime(format!("{what}: {e}"), 0)
}

fn parse_button(s: &str) -> Button {
    match s.to_ascii_lowercase().as_str() {
        "right" | "r" | "secondary" => Button::Right,
        "middle" | "m" | "wheel" => Button::Middle,
        _ => Button::Left,
    }
}

fn arg_str(args: &[StrykeValue], idx: usize) -> String {
    args.get(idx)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
}

fn arg_int(args: &[StrykeValue], idx: usize, default: i64) -> i64 {
    args.get(idx).map(|v| v.to_int()).unwrap_or(default)
}

fn arg_float(args: &[StrykeValue], idx: usize, default: f64) -> f64 {
    args.get(idx).map(|v| v.to_number()).unwrap_or(default)
}

/// True iff `args[idx]` is present and not `undef` — used to decide
/// whether an optional positional arg was actually passed.
fn has_arg(args: &[StrykeValue], idx: usize) -> bool {
    args.get(idx).is_some_and(|v| !v.is_undef())
}

// ─────────────────────────────────────────────────────────────────
// Key name table — covers PyAutoGUI's `KEYBOARD_KEYS` list plus the
// platform-specific extras enigo 0.6 exposes (left/right modifier
// variants, browser keys, media keys, language keys, num-pad keys).
// All lookups are ASCII-lowercase; single-char names fall through to
// `Key::Unicode(c)` so callers can do `key_press("a")` without us
// enumerating every letter / digit / punctuation.
// ─────────────────────────────────────────────────────────────────

const KEYBOARD_KEY_NAMES: &[&str] = &[
    // Whitespace / edit
    "tab", "enter", "return", "space", "backspace", "delete", "del",
    "escape", "esc", "linefeed",
    // Modifiers — generic + left/right variants
    "shift", "shiftleft", "shiftright",
    "ctrl", "control", "ctrlleft", "ctrlright",
    "alt", "altleft", "altright", "option", "optionleft", "optionright",
    "meta", "cmd", "command", "super", "win", "winleft", "winright",
    "rcommand",
    // Arrows + navigation
    "up", "down", "left", "right",
    "home", "end", "pageup", "pagedown", "pgup", "pgdn",
    "insert",
    // Locks
    "capslock", "numlock", "scrolllock", "shiftlock",
    // System
    "pause", "printscreen", "prntscrn", "prtsc", "prtscr", "print",
    "snapshot", "sleep", "power", "eject", "help", "apps", "clear",
    "select", "execute", "cancel", "fn", "function",
    "accept", "convert", "nonconvert", "modechange", "final",
    "find", "redo", "undo",
    // Function keys F1..F24
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10",
    "f11", "f12", "f13", "f14", "f15", "f16", "f17", "f18", "f19", "f20",
    "f21", "f22", "f23", "f24",
    // Numpad
    "num0", "num1", "num2", "num3", "num4", "num5", "num6", "num7",
    "num8", "num9",
    "add", "subtract", "multiply", "divide", "decimal", "separator",
    "numpadenter",
    // Media
    "volumeup", "volumedown", "volumemute",
    "playpause", "nexttrack", "prevtrack", "stop",
    "mediarewind", "mediafast", "mediaplay", "micmute",
    // Launch
    "launchmail", "launchmediaselect", "launchapp1", "launchapp2",
    // Browser
    "browserback", "browserforward", "browserrefresh", "browserstop",
    "browsersearch", "browserfavorites", "browserhome",
    // CJK input method
    "hangul", "hanguel", "hanja", "junja", "kana", "kanji",
    // Yen — Unicode passthrough
    "yen",
];

fn parse_key(name: &str) -> StrykeResult<Key> {
    let lc = name.to_ascii_lowercase();
    if let Some(k) = parse_key_common(&lc) {
        return Ok(k);
    }
    if let Some(k) = parse_key_platform(&lc) {
        return Ok(k);
    }
    if name.chars().count() == 1 {
        return Ok(Key::Unicode(name.chars().next().unwrap()));
    }
    Err(StrykeError::runtime(
        format!("key_press/down/up: unrecognized key name '{name}' on this platform"),
        0,
    ))
}

/// Variants that enigo exposes on every supported OS (macOS, Linux,
/// Windows). Anything platform-conditional lives in
/// `parse_key_platform` below.
fn parse_key_common(lc: &str) -> Option<Key> {
    use Key::*;
    Some(match lc {
        // ── modifiers ──
        "shift" => Shift,
        "shiftleft" => LShift,
        "shiftright" => RShift,
        "ctrl" | "control" => Control,
        "ctrlleft" => LControl,
        "ctrlright" => RControl,
        "alt" | "option" => Alt,
        "meta" | "cmd" | "command" | "super" => Meta,
        // ── whitespace + edit ──
        "return" | "enter" | "numpadenter" => Return,
        "tab" => Tab,
        "space" => Space,
        "backspace" => Backspace,
        "delete" | "del" => Delete,
        "escape" | "esc" => Escape,
        // ── arrows + nav ──
        "up" => UpArrow,
        "down" => DownArrow,
        "left" => LeftArrow,
        "right" => RightArrow,
        "home" => Home,
        "end" => End,
        "pageup" | "pgup" => PageUp,
        "pagedown" | "pgdn" => PageDown,
        // ── locks ──
        "capslock" => CapsLock,
        // ── system ──
        "help" => Help,
        // ── function keys F1..F24 ──
        "f1" => F1, "f2" => F2, "f3" => F3, "f4" => F4, "f5" => F5,
        "f6" => F6, "f7" => F7, "f8" => F8, "f9" => F9, "f10" => F10,
        "f11" => F11, "f12" => F12, "f13" => F13, "f14" => F14, "f15" => F15,
        "f16" => F16, "f17" => F17, "f18" => F18, "f19" => F19, "f20" => F20,
        // ── numpad ──
        "num0" => Numpad0, "num1" => Numpad1, "num2" => Numpad2, "num3" => Numpad3,
        "num4" => Numpad4, "num5" => Numpad5, "num6" => Numpad6, "num7" => Numpad7,
        "num8" => Numpad8, "num9" => Numpad9,
        "add" => Add,
        "subtract" => Subtract,
        "multiply" => Multiply,
        "divide" => Divide,
        "decimal" => Decimal,
        // ── media ──
        "volumeup" => VolumeUp,
        "volumedown" => VolumeDown,
        "volumemute" => VolumeMute,
        "playpause" | "mediaplay" => MediaPlayPause,
        "nexttrack" => MediaNextTrack,
        "prevtrack" => MediaPrevTrack,
        // ── yen as Unicode passthrough ──
        "yen" => Unicode('¥'),
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
fn parse_key_platform(lc: &str) -> Option<Key> {
    use Key::*;
    Some(match lc {
        // macOS-specific modifiers and synonyms
        "fn" | "function" => Function,
        "rcommand" | "rcmd" => RCommand,
        "roption" | "optionright" | "altright" => ROption,
        // Mac power / hardware
        "eject" => Eject,
        "power" => Power,
        "brightnessup" => BrightnessUp,
        "brightnessdown" => BrightnessDown,
        "contrastup" => ContrastUp,
        "contrastdown" => ContrastDown,
        "illuminationup" => IlluminationUp,
        "illuminationdown" => IlluminationDown,
        "illuminationtoggle" => IlluminationToggle,
        "launchpanel" => LaunchPanel,
        "launchpad" => Launchpad,
        "missioncontrol" => MissionControl,
        "mediarewind" => MediaRewind,
        "mediafast" => MediaFast,
        "vidmirror" => VidMirror,
        // On macOS the left/right Alt and Win variants don't exist
        // (the platform uses Option / RCommand instead). Map the
        // PyAutoGUI spellings to the closest macOS equivalent so
        // cross-platform scripts keep working.
        "altleft" | "optionleft" => Alt,
        "winleft" | "win" => Meta,
        "winright" => RCommand,
        _ => return None,
    })
}

#[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
fn parse_key_platform(lc: &str) -> Option<Key> {
    use Key::*;
    Some(match lc {
        // ── Available on BOTH Windows + Linux (enigo gates them
        // under cfg(any(windows, all(unix, not(macos))))) ──
        "f21" => F21, "f22" => F22, "f23" => F23, "f24" => F24,
        "printscreen" | "prntscrn" | "prtsc" | "prtscr" => PrintScr,
        "altleft" | "optionleft" => LMenu,
        "insert" => Insert,
        "numlock" => Numlock,
        "pause" => Pause,
        "modechange" => ModeChange,
        "select" => Select,
        "execute" => Execute,
        "cancel" => Cancel,
        "clear" => Clear,
        "stop" => MediaStop,
        "hangul" | "hanguel" => Hangul,
        "hanja" => Hanja,
        "kanji" => Kanji,
        // ── Windows-only variants ── enigo gates these under
        // cfg(target_os = "windows"); Linux build doesn't see them.
        // Per-arm cfg attrs keep one match block instead of
        // splitting into a separate parse_key_windows fn.
        #[cfg(target_os = "windows")] "altright" | "optionright" => RMenu,
        #[cfg(target_os = "windows")] "win" | "winleft" => LWin,
        #[cfg(target_os = "windows")] "winright" => RWin,
        #[cfg(target_os = "windows")] "apps" => Apps,
        #[cfg(target_os = "windows")] "sleep" => Sleep,
        #[cfg(target_os = "windows")] "accept" => Accept,
        #[cfg(target_os = "windows")] "convert" => Convert,
        #[cfg(target_os = "windows")] "nonconvert" => NonConvert,
        #[cfg(target_os = "windows")] "junja" => Junja,
        #[cfg(target_os = "windows")] "kana" => Kana,
        #[cfg(target_os = "windows")] "separator" => Separator,
        #[cfg(target_os = "windows")] "launchmail" => LaunchMail,
        #[cfg(target_os = "windows")] "launchmediaselect" => LaunchMediaSelect,
        #[cfg(target_os = "windows")] "launchapp1" => LaunchApp1,
        #[cfg(target_os = "windows")] "launchapp2" => LaunchApp2,
        #[cfg(target_os = "windows")] "browserback" => BrowserBack,
        #[cfg(target_os = "windows")] "browserforward" => BrowserForward,
        #[cfg(target_os = "windows")] "browserrefresh" => BrowserRefresh,
        #[cfg(target_os = "windows")] "browserstop" => BrowserStop,
        #[cfg(target_os = "windows")] "browsersearch" => BrowserSearch,
        #[cfg(target_os = "windows")] "browserfavorites" => BrowserFavorites,
        #[cfg(target_os = "windows")] "browserhome" => BrowserHome,
        _ => parse_key_unix(lc)?,
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn parse_key_unix(lc: &str) -> Option<Key> {
    use Key::*;
    Some(match lc {
        "shiftlock" => ShiftLock,
        "scrolllock" => ScrollLock,
        "linefeed" => Linefeed,
        "micmute" => MicMute,
        "find" => Find,
        "redo" => Redo,
        "undo" => Undo,
        // `Final` is Windows-only in enigo despite living next to
        // Function which is macOS-only — not a Linux variant.
        // Drop here; Windows path can still hit it via parse_key_platform
        // if a Windows-specific arm is added later.
        _ => return None,
    })
}

#[cfg(target_os = "windows")]
fn parse_key_unix(_lc: &str) -> Option<Key> {
    None
}

// ─────────────────────────────────────────────────────────────────
// Helpers: arg shapes and motion tweens
// ─────────────────────────────────────────────────────────────────

/// Parse an `[r, g, b]` array arg into a tuple. Accepts either a
/// stryke array `[200, 100, 50]` or three positional ints if the
/// caller spread it out.
fn parse_rgb(arg: &StrykeValue) -> StrykeResult<(u8, u8, u8)> {
    let v = arg.as_array_vec().ok_or_else(|| {
        StrykeError::runtime("color arg must be [r, g, b]", 0)
    })?;
    if v.len() < 3 {
        return Err(StrykeError::runtime(
            "color arg must have three components [r, g, b]",
            0,
        ));
    }
    let r = v[0].to_int().clamp(0, 255) as u8;
    let g = v[1].to_int().clamp(0, 255) as u8;
    let b = v[2].to_int().clamp(0, 255) as u8;
    Ok((r, g, b))
}

/// Animate the mouse from its current location to `(tx, ty)` over
/// `duration` seconds using linear interpolation at 60 fps. `duration
/// <= 0` is a single-step warp, matching pyautogui's default.
fn linear_move(
    e: &mut Enigo,
    tx: i32,
    ty: i32,
    duration: f64,
) -> Result<(), enigo::InputError> {
    if duration <= 0.0 {
        return e.move_mouse(tx, ty, Coordinate::Abs);
    }
    let (sx, sy) = e.location()?;
    let steps = ((duration * 60.0).max(2.0)) as i32;
    let step_dur = Duration::from_secs_f64(duration / steps as f64);
    let dx = (tx - sx) as f64;
    let dy = (ty - sy) as f64;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let x = sx as f64 + dx * t;
        let y = sy as f64 + dy * t;
        e.move_mouse(x as i32, y as i32, Coordinate::Abs)?;
        thread_sleep(step_dur);
    }
    Ok(())
}

/// Move to `(x, y)` before pressing a button — used by every click
/// variant when `x` / `y` are supplied positionally. Skips the move
/// entirely if `x` is absent (matches pyautogui's `click()` with no
/// coords: click at current position).
fn maybe_move_to(
    e: &mut Enigo,
    args: &[StrykeValue],
    x_idx: usize,
    y_idx: usize,
) -> Result<(), enigo::InputError> {
    if has_arg(args, x_idx) && has_arg(args, y_idx) {
        let x = args[x_idx].to_int() as i32;
        let y = args[y_idx].to_int() as i32;
        e.move_mouse(x, y, Coordinate::Abs)?;
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Position / size queries
// ─────────────────────────────────────────────────────────────────

pub fn builtin_mouse_pos(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let e = make_enigo()?;
    let (x, y) = e.location().map_err(|er| err(er, "mouse_pos"))?;
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(x as i64),
        StrykeValue::integer(y as i64),
    ]))
}

pub fn builtin_screen_size(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let e = make_enigo()?;
    let (w, h) = e.main_display().map_err(|er| err(er, "screen_size"))?;
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(w as i64),
        StrykeValue::integer(h as i64),
    ]))
}

pub fn builtin_on_screen(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    if args.len() < 2 {
        return Err(StrykeError::runtime("on_screen(X, Y) needs two coords", 0));
    }
    let x = args[0].to_int() as i32;
    let y = args[1].to_int() as i32;
    let e = make_enigo()?;
    let (w, h) = e.main_display().map_err(|er| err(er, "on_screen"))?;
    let inside = x >= 0 && y >= 0 && x < w && y < h;
    Ok(StrykeValue::integer(if inside { 1 } else { 0 }))
}

// ─────────────────────────────────────────────────────────────────
// Motion
// ─────────────────────────────────────────────────────────────────

pub fn builtin_mouse_move(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    if args.len() < 2 {
        return Err(StrykeError::runtime("mouse_move(X, Y, DURATION?) needs two coords", 0));
    }
    let x = args[0].to_int() as i32;
    let y = args[1].to_int() as i32;
    let duration = arg_float(args, 2, 0.0);
    let mut e = make_enigo()?;
    linear_move(&mut e, x, y, duration).map_err(|er| err(er, "mouse_move"))?;
    Ok(StrykeValue::integer(0))
}

pub fn builtin_mouse_move_rel(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    if args.len() < 2 {
        return Err(StrykeError::runtime("mouse_move_rel(DX, DY, DURATION?) needs two deltas", 0));
    }
    let dx = args[0].to_int() as i32;
    let dy = args[1].to_int() as i32;
    let duration = arg_float(args, 2, 0.0);
    let mut e = make_enigo()?;
    if duration <= 0.0 {
        e.move_mouse(dx, dy, Coordinate::Rel)
            .map_err(|er| err(er, "mouse_move_rel"))?;
    } else {
        let (sx, sy) = e.location().map_err(|er| err(er, "mouse_move_rel"))?;
        linear_move(&mut e, sx + dx, sy + dy, duration)
            .map_err(|er| err(er, "mouse_move_rel"))?;
    }
    Ok(StrykeValue::integer(0))
}

fn drag_to(
    args: &[StrykeValue],
    relative: bool,
    name: &'static str,
) -> StrykeResult<StrykeValue> {
    if args.len() < 2 {
        return Err(StrykeError::runtime(
            format!("{name}(X, Y, DURATION?, BUTTON?) needs two coords"),
            0,
        ));
    }
    let a = args[0].to_int() as i32;
    let b = args[1].to_int() as i32;
    let duration = arg_float(args, 2, 0.0);
    let button = args
        .get(3)
        .and_then(|v| v.as_str())
        .map(|s| parse_button(&s))
        .unwrap_or(Button::Left);
    let mut e = make_enigo()?;
    let (sx, sy) = e.location().map_err(|er| err(er, name))?;
    let (tx, ty) = if relative { (sx + a, sy + b) } else { (a, b) };
    e.button(button, Direction::Press).map_err(|er| err(er, name))?;
    let move_res = linear_move(&mut e, tx, ty, duration);
    // Always release even if the move errored — otherwise a stuck
    // mouse-button state survives the failure and the user has to
    // manually click to recover.
    let release_res = e.button(button, Direction::Release);
    move_res.map_err(|er| err(er, name))?;
    release_res.map_err(|er| err(er, name))?;
    Ok(StrykeValue::integer(0))
}

pub fn builtin_mouse_drag(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    drag_to(args, false, "mouse_drag")
}

pub fn builtin_mouse_drag_rel(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    drag_to(args, true, "mouse_drag_rel")
}

// ─────────────────────────────────────────────────────────────────
// Buttons
// ─────────────────────────────────────────────────────────────────

/// Internal: figure out (clicks, interval, button) for a click op
/// where the first two positional args MAY be coords. pyautogui's
/// `click(x, y, clicks, interval, button)` shape.
fn click_args(args: &[StrykeValue]) -> (i64, f64, Button) {
    let clicks = arg_int(args, 2, 1).max(1);
    let interval = arg_float(args, 3, 0.0).max(0.0);
    let button = args
        .get(4)
        .and_then(|v| v.as_str())
        .map(|s| parse_button(&s))
        .unwrap_or(Button::Left);
    (clicks, interval, button)
}

pub fn builtin_mouse_click(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (clicks, interval, button) = click_args(args);
    let mut e = make_enigo()?;
    maybe_move_to(&mut e, args, 0, 1).map_err(|er| err(er, "mouse_click"))?;
    for i in 0..clicks {
        e.button(button, Direction::Click)
            .map_err(|er| err(er, "mouse_click"))?;
        if interval > 0.0 && i + 1 < clicks {
            thread_sleep(Duration::from_secs_f64(interval));
        }
    }
    Ok(StrykeValue::integer(0))
}

fn click_n_at(args: &[StrykeValue], n: i64, button: Button, name: &'static str) -> StrykeResult<StrykeValue> {
    let mut e = make_enigo()?;
    maybe_move_to(&mut e, args, 0, 1).map_err(|er| err(er, name))?;
    for _ in 0..n {
        e.button(button, Direction::Click)
            .map_err(|er| err(er, name))?;
    }
    Ok(StrykeValue::integer(0))
}

pub fn builtin_mouse_right_click(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    click_n_at(args, 1, Button::Right, "mouse_right_click")
}

pub fn builtin_mouse_middle_click(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    click_n_at(args, 1, Button::Middle, "mouse_middle_click")
}

pub fn builtin_mouse_double_click(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let button = args
        .get(2)
        .and_then(|v| v.as_str())
        .map(|s| parse_button(&s))
        .unwrap_or(Button::Left);
    click_n_at(args, 2, button, "mouse_double_click")
}

pub fn builtin_mouse_triple_click(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let button = args
        .get(2)
        .and_then(|v| v.as_str())
        .map(|s| parse_button(&s))
        .unwrap_or(Button::Left);
    click_n_at(args, 3, button, "mouse_triple_click")
}

pub fn builtin_mouse_down(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let b = args
        .first()
        .and_then(|v| v.as_str())
        .map(|s| parse_button(&s))
        .unwrap_or(Button::Left);
    let mut e = make_enigo()?;
    e.button(b, Direction::Press)
        .map_err(|er| err(er, "mouse_down"))?;
    Ok(StrykeValue::integer(0))
}

pub fn builtin_mouse_up(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let b = args
        .first()
        .and_then(|v| v.as_str())
        .map(|s| parse_button(&s))
        .unwrap_or(Button::Left);
    let mut e = make_enigo()?;
    e.button(b, Direction::Release)
        .map_err(|er| err(er, "mouse_up"))?;
    Ok(StrykeValue::integer(0))
}

// ─────────────────────────────────────────────────────────────────
// Wheel
// ─────────────────────────────────────────────────────────────────

fn scroll_axis(args: &[StrykeValue], axis: Axis, name: &'static str) -> StrykeResult<StrykeValue> {
    let clicks = arg_int(args, 0, 0) as i32;
    let mut e = make_enigo()?;
    maybe_move_to(&mut e, args, 1, 2).map_err(|er| err(er, name))?;
    e.scroll(clicks, axis).map_err(|er| err(er, name))?;
    Ok(StrykeValue::integer(0))
}

pub fn builtin_mouse_scroll(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    scroll_axis(args, Axis::Vertical, "mouse_scroll")
}

pub fn builtin_mouse_hscroll(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    scroll_axis(args, Axis::Horizontal, "mouse_hscroll")
}

// ─────────────────────────────────────────────────────────────────
// Keyboard
// ─────────────────────────────────────────────────────────────────

pub fn builtin_key_press(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let name = arg_str(args, 0);
    let presses = arg_int(args, 1, 1).max(1);
    let interval = arg_float(args, 2, 0.0).max(0.0);
    let key = parse_key(&name)?;
    let mut e = make_enigo()?;
    for i in 0..presses {
        e.key(key, Direction::Click)
            .map_err(|er| err(er, "key_press"))?;
        if interval > 0.0 && i + 1 < presses {
            thread_sleep(Duration::from_secs_f64(interval));
        }
    }
    Ok(StrykeValue::integer(0))
}

pub fn builtin_key_down(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let name = arg_str(args, 0);
    let key = parse_key(&name)?;
    let mut e = make_enigo()?;
    e.key(key, Direction::Press)
        .map_err(|er| err(er, "key_down"))?;
    Ok(StrykeValue::integer(0))
}

pub fn builtin_key_up(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let name = arg_str(args, 0);
    let key = parse_key(&name)?;
    let mut e = make_enigo()?;
    e.key(key, Direction::Release)
        .map_err(|er| err(er, "key_up"))?;
    Ok(StrykeValue::integer(0))
}

pub fn builtin_key_type(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let text = arg_str(args, 0);
    let interval = arg_float(args, 1, 0.0).max(0.0);
    let mut e = make_enigo()?;
    if interval <= 0.0 {
        e.text(&text).map_err(|er| err(er, "key_type"))?;
    } else {
        // Per-char dispatch so the inter-char delay applies. Walk
        // by Unicode scalar — `text()` on a 1-char string takes the
        // OS-keyboard-layout-correct fast path on every platform.
        let mut iter = text.chars().peekable();
        while let Some(c) = iter.next() {
            let one = c.to_string();
            e.text(&one).map_err(|er| err(er, "key_type"))?;
            if iter.peek().is_some() {
                thread_sleep(Duration::from_secs_f64(interval));
            }
        }
    }
    Ok(StrykeValue::integer(0))
}

pub fn builtin_key_hotkey(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Err(StrykeError::runtime(
            "key_hotkey needs at least one key",
            0,
        ));
    }
    // Detect a trailing `interval => SECS` numeric arg pattern:
    // pyautogui's signature is `hotkey(*keys, interval=0)`. We treat
    // a trailing float arg whose value is fractional or zero as the
    // interval; otherwise every arg is a key name.
    let (key_args, interval) = match args.last() {
        Some(v) if !v.as_str().is_some_and(|s| !s.is_empty()) => {
            let i = v.to_number().max(0.0);
            (&args[..args.len() - 1], i)
        }
        _ => (args, 0.0_f64),
    };
    // Flatten one level if a single array arg was passed:
    // `key_hotkey(@chord)` or `key_hotkey(["ctrl","c"])`.
    let names: Vec<String> = if key_args.len() == 1 {
        match key_args[0].as_array_vec() {
            Some(vs) => vs.iter().map(|v| v.as_str().unwrap_or_default()).collect(),
            None => vec![key_args[0].as_str().unwrap_or_default()],
        }
    } else {
        key_args.iter().map(|v| v.as_str().unwrap_or_default()).collect()
    };
    let keys: StrykeResult<Vec<Key>> = names.iter().map(|n| parse_key(n)).collect();
    let keys = keys?;
    let mut e = make_enigo()?;
    for (i, k) in keys.iter().enumerate() {
        e.key(*k, Direction::Press)
            .map_err(|er| err(er, "key_hotkey/press"))?;
        if interval > 0.0 && i + 1 < keys.len() {
            thread_sleep(Duration::from_secs_f64(interval));
        }
    }
    for k in keys.iter().rev() {
        e.key(*k, Direction::Release)
            .map_err(|er| err(er, "key_hotkey/release"))?;
    }
    Ok(StrykeValue::integer(0))
}

pub fn builtin_keyboard_keys(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v: Vec<StrykeValue> = KEYBOARD_KEY_NAMES
        .iter()
        .map(|n| StrykeValue::string((*n).to_string()))
        .collect();
    Ok(StrykeValue::array(v))
}

// ─────────────────────────────────────────────────────────────────
// Pixel + screenshot (xcap-backed)
// ─────────────────────────────────────────────────────────────────

fn primary_monitor() -> StrykeResult<xcap::Monitor> {
    let mons = xcap::Monitor::all()
        .map_err(|e| StrykeError::runtime(format!("display enumeration failed: {e}"), 0))?;
    mons.into_iter().next().ok_or_else(|| {
        StrykeError::runtime("no displays detected", 0)
    })
}

/// Capture the primary display as an RGBA image, then read a single
/// pixel. macOS prompts for Screen Recording on first call.
pub fn builtin_pixel(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    if args.len() < 2 {
        return Err(StrykeError::runtime("pixel(X, Y) needs two coords", 0));
    }
    let x = args[0].to_int() as u32;
    let y = args[1].to_int() as u32;
    let mon = primary_monitor()?;
    let img = mon
        .capture_image()
        .map_err(|e| err(e, "pixel/capture"))?;
    if x >= img.width() || y >= img.height() {
        return Err(StrykeError::runtime(
            format!("pixel({x}, {y}) out of bounds {}x{}", img.width(), img.height()),
            0,
        ));
    }
    let p = img.get_pixel(x, y);
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(p[0] as i64),
        StrykeValue::integer(p[1] as i64),
        StrykeValue::integer(p[2] as i64),
    ]))
}

pub fn builtin_pixel_matches_color(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    if args.len() < 3 {
        return Err(StrykeError::runtime(
            "pixel_matches_color(X, Y, [R,G,B], TOLERANCE?) needs three args",
            0,
        ));
    }
    let x = args[0].to_int() as u32;
    let y = args[1].to_int() as u32;
    let (tr, tg, tb) = parse_rgb(&args[2])?;
    let tol = arg_int(args, 3, 0).max(0) as i32;
    let mon = primary_monitor()?;
    let img = mon
        .capture_image()
        .map_err(|e| err(e, "pixel_matches_color/capture"))?;
    if x >= img.width() || y >= img.height() {
        return Ok(StrykeValue::integer(0));
    }
    let p = img.get_pixel(x, y);
    let dr = (p[0] as i32 - tr as i32).abs();
    let dg = (p[1] as i32 - tg as i32).abs();
    let db = (p[2] as i32 - tb as i32).abs();
    let ok = dr <= tol && dg <= tol && db <= tol;
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

/// Capture the primary display.
///   `screenshot()`           → returns `[w, h, RGBA-bytes]` triple.
///   `screenshot("foo.png")`  → writes PNG to disk; returns the path.
pub fn builtin_screenshot(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mon = primary_monitor()?;
    let img = mon.capture_image().map_err(|e| err(e, "screenshot/capture"))?;
    write_screenshot_result(&img, args.first())
}

/// Capture a `(L, T, W, H)` region of the primary display.
pub fn builtin_screenshot_region(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    if args.len() < 4 {
        return Err(StrykeError::runtime(
            "screenshot_region(L, T, W, H, PATH?) needs four region coords",
            0,
        ));
    }
    let l = args[0].to_int() as i32;
    let t = args[1].to_int() as i32;
    let w = args[2].to_int() as u32;
    let h = args[3].to_int() as u32;
    let mon = primary_monitor()?;
    let full = mon
        .capture_image()
        .map_err(|e| err(e, "screenshot_region/capture"))?;
    // xcap returns full-display RGBA; crop with the `image` crate.
    let cropped = image::imageops::crop_imm(&full, l.max(0) as u32, t.max(0) as u32, w, h)
        .to_image();
    write_screenshot_result(&cropped, args.get(4))
}

fn write_screenshot_result(
    img: &image::RgbaImage,
    path_arg: Option<&StrykeValue>,
) -> StrykeResult<StrykeValue> {
    if let Some(p) = path_arg.and_then(|v| v.as_str()) {
        img.save(&p).map_err(|e| err(e, "screenshot/save"))?;
        return Ok(StrykeValue::string(p));
    }
    let (w, h) = (img.width(), img.height());
    let bytes: Vec<StrykeValue> = img
        .as_raw()
        .iter()
        .map(|b| StrykeValue::integer(*b as i64))
        .collect();
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(w as i64),
        StrykeValue::integer(h as i64),
        StrykeValue::array(bytes),
    ]))
}
