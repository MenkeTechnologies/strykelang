//! ZLE Keymap management

use std::collections::HashMap;

/// Keymap identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeymapName {
    Emacs,
    ViInsert,
    ViCommand,
    Main,       // alias for current main keymap
    Isearch,    // incremental search
    Command,    // command mode
    MenuSelect, // menu selection
}

impl KeymapName {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "emacs" => Some(Self::Emacs),
            "viins" => Some(Self::ViInsert),
            "vicmd" => Some(Self::ViCommand),
            "main" => Some(Self::Main),
            "isearch" => Some(Self::Isearch),
            "command" => Some(Self::Command),
            "menuselect" => Some(Self::MenuSelect),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Emacs => "emacs",
            Self::ViInsert => "viins",
            Self::ViCommand => "vicmd",
            Self::Main => "main",
            Self::Isearch => "isearch",
            Self::Command => "command",
            Self::MenuSelect => "menuselect",
        }
    }
}

/// A keymap - mapping from key sequences to widget names
#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: HashMap<String, String>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new()
    }
}

impl Keymap {
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    /// Create default emacs keymap
    pub fn emacs_default() -> Self {
        let mut km = Self::new();

        // Movement
        km.bind("^F", "forward-char");
        km.bind("^B", "backward-char");
        km.bind("^A", "beginning-of-line");
        km.bind("^E", "end-of-line");
        km.bind("\\ef", "forward-word"); // Alt-f
        km.bind("\\eb", "backward-word"); // Alt-b

        // Editing
        km.bind("^D", "delete-char");
        km.bind("^H", "backward-delete-char");
        km.bind("^?", "backward-delete-char"); // Backspace
        km.bind("^K", "kill-line");
        km.bind("^U", "backward-kill-line");
        km.bind("\\ed", "kill-word"); // Alt-d
        km.bind("\\e^?", "backward-kill-word"); // Alt-Backspace
        km.bind("^W", "backward-kill-word");
        km.bind("^Y", "yank");
        km.bind("\\ey", "yank-pop"); // Alt-y

        // Undo
        km.bind("^_", "undo");
        km.bind("^X^U", "undo");
        km.bind("\\e_", "redo"); // Alt-_

        // History
        km.bind("^P", "up-line-or-history");
        km.bind("^N", "down-line-or-history");
        km.bind("\\e<", "beginning-of-history");
        km.bind("\\e>", "end-of-history");
        km.bind("^R", "history-incremental-search-backward");
        km.bind("^S", "history-incremental-search-forward");

        // Completion
        km.bind("^I", "expand-or-complete"); // Tab
        km.bind("\\e\\e", "complete-word");

        // Accept/misc
        km.bind("^J", "accept-line"); // Enter
        km.bind("^M", "accept-line"); // Enter
        km.bind("^G", "send-break");
        km.bind("^C", "send-break");
        km.bind("^L", "clear-screen");

        // Transpose
        km.bind("^T", "transpose-chars");
        km.bind("\\et", "transpose-words");

        // Case
        km.bind("\\ec", "capitalize-word");
        km.bind("\\el", "down-case-word");
        km.bind("\\eu", "up-case-word");

        // Region
        km.bind("^@", "set-mark-command"); // Ctrl-Space
        km.bind("^X^X", "exchange-point-and-mark");
        km.bind("\\ew", "copy-region-as-kill");

        km
    }

    /// Create default vi insert mode keymap
    pub fn viins_default() -> Self {
        let mut km = Self::new();

        // Enter command mode
        km.bind("^[", "vi-cmd-mode"); // Escape

        // Basic editing (same as emacs)
        km.bind("^H", "backward-delete-char");
        km.bind("^?", "backward-delete-char");
        km.bind("^W", "backward-kill-word");
        km.bind("^U", "backward-kill-line");

        // Accept
        km.bind("^J", "accept-line");
        km.bind("^M", "accept-line");

        // Completion
        km.bind("^I", "expand-or-complete");

        // History
        km.bind("^P", "up-line-or-history");
        km.bind("^N", "down-line-or-history");

        km
    }

    /// Create default vi command mode keymap
    pub fn vicmd_default() -> Self {
        let mut km = Self::new();

        // Enter insert mode
        km.bind("i", "vi-insert");
        km.bind("a", "vi-add-next");
        km.bind("I", "vi-insert-bol");
        km.bind("A", "vi-add-eol");

        // Movement
        km.bind("h", "backward-char");
        km.bind("l", "forward-char");
        km.bind("w", "forward-word");
        km.bind("b", "backward-word");
        km.bind("0", "beginning-of-line");
        km.bind("^", "beginning-of-line");
        km.bind("$", "end-of-line");

        // Delete
        km.bind("x", "delete-char");
        km.bind("X", "backward-delete-char");
        km.bind("dd", "kill-whole-line");
        km.bind("dw", "kill-word");
        km.bind("db", "backward-kill-word");
        km.bind("d$", "kill-line");
        km.bind("d0", "backward-kill-line");

        // Yank/paste
        km.bind("y", "vi-yank");
        km.bind("p", "vi-put-after");
        km.bind("P", "vi-put-before");

        // History
        km.bind("k", "up-line-or-history");
        km.bind("j", "down-line-or-history");
        km.bind("/", "history-incremental-search-backward");
        km.bind("?", "history-incremental-search-forward");
        km.bind("n", "vi-repeat-search");
        km.bind("N", "vi-rev-repeat-search");

        // Undo
        km.bind("u", "undo");
        km.bind("^R", "redo");

        // Accept
        km.bind("^J", "accept-line");
        km.bind("^M", "accept-line");

        km
    }

    /// Bind a key sequence to a widget
    pub fn bind(&mut self, keys: &str, widget: &str) {
        let normalized = Self::normalize_keys(keys);
        self.bindings.insert(normalized, widget.to_string());
    }

    /// Unbind a key sequence
    pub fn unbind(&mut self, keys: &str) {
        let normalized = Self::normalize_keys(keys);
        self.bindings.remove(&normalized);
    }

    /// Look up a key sequence
    pub fn lookup(&self, keys: &str) -> Option<&str> {
        let normalized = Self::normalize_keys(keys);
        self.bindings.get(&normalized).map(|s| s.as_str())
    }

    /// Check if keys could be a prefix of a binding
    pub fn has_prefix(&self, keys: &str) -> bool {
        let normalized = Self::normalize_keys(keys);
        self.bindings
            .keys()
            .any(|k| k.starts_with(&normalized) && k != &normalized)
    }

    /// List all bindings
    pub fn list_bindings(&self) -> impl Iterator<Item = (&str, &str)> {
        self.bindings.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Normalize key notation
    /// Converts various formats to a canonical form:
    /// ^X -> control-X
    /// \eX -> escape X (meta)
    /// \C-x -> control-x
    /// \M-x -> meta-x
    fn normalize_keys(keys: &str) -> String {
        let mut result = String::new();
        let mut chars = keys.chars().peekable();

        while let Some(c) = chars.next() {
            match c {
                '^' => {
                    // Control character
                    if let Some(&next) = chars.peek() {
                        chars.next();
                        let ctrl_char = if next == '?' {
                            '\x7f' // DEL
                        } else if next == '@' {
                            '\x00' // NUL
                        } else if next == '[' {
                            '\x1b' // ESC
                        } else {
                            // Ctrl-A through Ctrl-Z, etc.
                            ((next.to_ascii_uppercase() as u8) & 0x1f) as char
                        };
                        result.push(ctrl_char);
                    } else {
                        result.push(c);
                    }
                }
                '\\' => {
                    // Escape sequence
                    if let Some(&next) = chars.peek() {
                        match next {
                            'e' | 'E' => {
                                chars.next();
                                result.push('\x1b'); // ESC
                            }
                            'C' => {
                                chars.next();
                                if chars.peek() == Some(&'-') {
                                    chars.next();
                                    if let Some(&ctrl_char) = chars.peek() {
                                        chars.next();
                                        let ctrl =
                                            ((ctrl_char.to_ascii_uppercase() as u8) & 0x1f) as char;
                                        result.push(ctrl);
                                    }
                                }
                            }
                            'M' => {
                                chars.next();
                                if chars.peek() == Some(&'-') {
                                    chars.next();
                                    result.push('\x1b'); // ESC prefix for meta
                                    if let Some(&meta_char) = chars.peek() {
                                        chars.next();
                                        result.push(meta_char);
                                    }
                                }
                            }
                            'n' => {
                                chars.next();
                                result.push('\n');
                            }
                            't' => {
                                chars.next();
                                result.push('\t');
                            }
                            'r' => {
                                chars.next();
                                result.push('\r');
                            }
                            '\\' => {
                                chars.next();
                                result.push('\\');
                            }
                            _ => {
                                result.push(c);
                            }
                        }
                    } else {
                        result.push(c);
                    }
                }
                _ => result.push(c),
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_keys() {
        assert_eq!(Keymap::normalize_keys("^A"), "\x01");
        assert_eq!(Keymap::normalize_keys("^?"), "\x7f");
        assert_eq!(Keymap::normalize_keys("\\ef"), "\x1bf");
        assert_eq!(Keymap::normalize_keys("\\C-a"), "\x01");
        assert_eq!(Keymap::normalize_keys("\\M-x"), "\x1bx");
    }

    #[test]
    fn test_keymap_bind_lookup() {
        let mut km = Keymap::new();
        km.bind("^A", "beginning-of-line");

        assert_eq!(km.lookup("^A"), Some("beginning-of-line"));
        assert_eq!(km.lookup("\x01"), Some("beginning-of-line"));
    }

    #[test]
    fn test_has_prefix() {
        let mut km = Keymap::new();
        km.bind("^X^U", "undo");

        assert!(km.has_prefix("^X"));
        assert!(!km.has_prefix("^X^U"));
        assert!(!km.has_prefix("^A"));
    }
}
