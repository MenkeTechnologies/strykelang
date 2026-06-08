//! GUI automation builtins — pyautogui-equivalent surface backed by
//! the `enigo` crate. Cross-platform (macOS CGEvent, X11/Wayland on
//! Linux, SendInput on Windows). No external binaries; everything
//! goes through libc/CoreFoundation/X-protocol/Win32.
//!
//! Surface (dispatched from `builtins.rs`):
//!
//!   mouse_move(x, y)         — absolute move
//!   mouse_move_rel(dx, dy)   — relative move
//!   mouse_pos()              — returns `[x, y]`
//!   mouse_click(button?)     — "left" | "right" | "middle" (default left)
//!   mouse_down(button?)
//!   mouse_up(button?)
//!   mouse_scroll(dy)         — positive = up, negative = down
//!   key_press(name)          — "a", "Return", "F5", "ctrl", etc.
//!   key_down(name)
//!   key_up(name)
//!   key_type(text)           — type a literal string (utf-8 aware)
//!   key_hotkey(@keys)        — chord like `("ctrl", "c")`; press in
//!                              order, release in reverse.
//!   screen_size()            — returns `[w, h]` for the primary display.
//!
//! Returns are integers (0 on success) or arrays for queries. Errors
//! propagate via `StrykeError::runtime` — macOS accessibility-permission
//! denials, Wayland-portal failures, and unknown key names all land
//! there.
//!
//! Platform notes: on macOS the first invocation prompts the user
//! to grant Accessibility access to whichever terminal/IDE launched
//! stryke. Subsequent calls run silently. On Wayland (Sway, GNOME)
//! synthetic input is only allowed via the `wlroots-virtual-pointer`
//! or `libei` portals; enigo returns `InputError` if neither is
//! available. X11 always works.

use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;

use enigo::{
    Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings,
};

fn make_enigo() -> StrykeResult<Enigo> {
    Enigo::new(&Settings::default())
        .map_err(|e| StrykeError::runtime(format!("GUI init failed: {e}"), 0))
}

fn parse_button(s: &str) -> Button {
    match s.to_ascii_lowercase().as_str() {
        "right" | "r" => Button::Right,
        "middle" | "m" => Button::Middle,
        _ => Button::Left,
    }
}

/// Map a user-supplied key name (`"Return"`, `"ctrl"`, `"F5"`, `"a"`)
/// to an `enigo::Key`. Case-insensitive on the symbolic names. A
/// single character falls through to `Key::Unicode(char)` so callers
/// can do `key_press("a")` without enumerating every letter.
fn parse_key(name: &str) -> StrykeResult<Key> {
    let k = match name {
        // Modifiers — all common spellings
        "shift" | "Shift" | "SHIFT" => Key::Shift,
        "ctrl" | "Ctrl" | "CTRL" | "control" | "Control" => Key::Control,
        "alt" | "Alt" | "ALT" | "option" | "Option" => Key::Alt,
        "meta" | "Meta" | "cmd" | "Cmd" | "CMD" | "command" | "Command"
        | "super" | "Super" | "win" | "Win" => Key::Meta,
        // Whitespace + edit
        "Return" | "return" | "Enter" | "enter" | "RETURN" => Key::Return,
        "Tab" | "tab" | "TAB" => Key::Tab,
        "Space" | "space" | "SPACE" => Key::Space,
        "Backspace" | "backspace" | "BS" => Key::Backspace,
        "Delete" | "delete" | "Del" | "DEL" => Key::Delete,
        "Escape" | "escape" | "Esc" | "esc" | "ESC" => Key::Escape,
        // Arrows + nav
        "Up" | "up" | "UpArrow" => Key::UpArrow,
        "Down" | "down" | "DownArrow" => Key::DownArrow,
        "Left" | "left" | "LeftArrow" => Key::LeftArrow,
        "Right" | "right" | "RightArrow" => Key::RightArrow,
        "Home" | "home" => Key::Home,
        "End" | "end" => Key::End,
        "PageUp" | "pageup" | "PgUp" => Key::PageUp,
        "PageDown" | "pagedown" | "PgDn" => Key::PageDown,
        // Function keys
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        // Single-char fallback — pass through as Unicode.
        // pyautogui's `press('a')` style works without enumerating
        // every letter / digit / punctuation.
        _ if name.chars().count() == 1 => Key::Unicode(name.chars().next().unwrap()),
        _ => {
            return Err(StrykeError::runtime(
                format!("key_press/down/up: unrecognized key name '{name}'"),
                0,
            ));
        }
    };
    Ok(k)
}

fn err<E: std::fmt::Display>(e: E, what: &str) -> StrykeError {
    StrykeError::runtime(format!("{what}: {e}"), 0)
}

pub fn builtin_mouse_move(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    if args.len() < 2 {
        return Err(StrykeError::runtime("mouse_move(X, Y) needs two coords", 0));
    }
    let x = args[0].to_int() as i32;
    let y = args[1].to_int() as i32;
    let mut e = make_enigo()?;
    e.move_mouse(x, y, Coordinate::Abs)
        .map_err(|er| err(er, "mouse_move"))?;
    Ok(StrykeValue::integer(0))
}

pub fn builtin_mouse_move_rel(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    if args.len() < 2 {
        return Err(StrykeError::runtime(
            "mouse_move_rel(DX, DY) needs two deltas",
            0,
        ));
    }
    let dx = args[0].to_int() as i32;
    let dy = args[1].to_int() as i32;
    let mut e = make_enigo()?;
    e.move_mouse(dx, dy, Coordinate::Rel)
        .map_err(|er| err(er, "mouse_move_rel"))?;
    Ok(StrykeValue::integer(0))
}

pub fn builtin_mouse_pos(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let e = make_enigo()?;
    let (x, y) = e.location().map_err(|er| err(er, "mouse_pos"))?;
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(x as i64),
        StrykeValue::integer(y as i64),
    ]))
}

pub fn builtin_mouse_click(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let b = args
        .first()
        .and_then(|v| v.as_str())
        .map(|s| parse_button(&s))
        .unwrap_or(Button::Left);
    let mut e = make_enigo()?;
    e.button(b, Direction::Click)
        .map_err(|er| err(er, "mouse_click"))?;
    Ok(StrykeValue::integer(0))
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

pub fn builtin_mouse_scroll(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let dy = args.first().map(|v| v.to_int()).unwrap_or(0) as i32;
    let mut e = make_enigo()?;
    e.scroll(dy, Axis::Vertical)
        .map_err(|er| err(er, "mouse_scroll"))?;
    Ok(StrykeValue::integer(0))
}

fn arg_str(args: &[StrykeValue], idx: usize) -> String {
    args.get(idx)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
}

pub fn builtin_key_press(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let name = arg_str(args, 0);
    let key = parse_key(&name)?;
    let mut e = make_enigo()?;
    e.key(key, Direction::Click)
        .map_err(|er| err(er, "key_press"))?;
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
    let mut e = make_enigo()?;
    e.text(&text).map_err(|er| err(er, "key_type"))?;
    Ok(StrykeValue::integer(0))
}

pub fn builtin_key_hotkey(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Err(StrykeError::runtime(
            "key_hotkey needs at least one key",
            0,
        ));
    }
    // Flatten one level — if a single array arg was passed
    // (`key_hotkey(@chord)`), use its elements; otherwise treat
    // positional args as the chord directly.
    let names: Vec<String> = if args.len() == 1 {
        match args[0].as_array_vec() {
            Some(vs) => vs.iter().map(|v| v.as_str().unwrap_or_default()).collect(),
            None => vec![args[0].as_str().unwrap_or_default()],
        }
    } else {
        args.iter().map(|v| v.as_str().unwrap_or_default()).collect()
    };
    let keys: StrykeResult<Vec<Key>> = names.iter().map(|n| parse_key(n)).collect();
    let keys = keys?;
    let mut e = make_enigo()?;
    for k in &keys {
        e.key(*k, Direction::Press)
            .map_err(|er| err(er, "key_hotkey/press"))?;
    }
    for k in keys.iter().rev() {
        e.key(*k, Direction::Release)
            .map_err(|er| err(er, "key_hotkey/release"))?;
    }
    Ok(StrykeValue::integer(0))
}

pub fn builtin_screen_size(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let e = make_enigo()?;
    let (w, h) = e.main_display().map_err(|er| err(er, "screen_size"))?;
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(w as i64),
        StrykeValue::integer(h as i64),
    ]))
}
