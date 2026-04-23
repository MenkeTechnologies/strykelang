//! ZLE utility functions
//!
//! Direct port from zsh/Src/Zle/zle_utils.c

use super::main::{Zle, ZleChar, ZleString};

impl Zle {
    /// Insert string at cursor position
    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.zleline.insert(self.zlecs, c);
            self.zlecs += 1;
            self.zlell += 1;
        }
        self.resetneeded = true;
    }

    /// Insert chars at cursor position
    pub fn insert_chars(&mut self, chars: &[ZleChar]) {
        for &c in chars {
            self.zleline.insert(self.zlecs, c);
            self.zlecs += 1;
            self.zlell += 1;
        }
        self.resetneeded = true;
    }

    /// Delete n characters at cursor position
    pub fn delete_chars(&mut self, n: usize) {
        let n = n.min(self.zlell - self.zlecs);
        for _ in 0..n {
            if self.zlecs < self.zlell {
                self.zleline.remove(self.zlecs);
                self.zlell -= 1;
            }
        }
        self.resetneeded = true;
    }

    /// Delete n characters before cursor
    pub fn backspace_chars(&mut self, n: usize) {
        let n = n.min(self.zlecs);
        for _ in 0..n {
            if self.zlecs > 0 {
                self.zlecs -= 1;
                self.zleline.remove(self.zlecs);
                self.zlell -= 1;
            }
        }
        self.resetneeded = true;
    }

    /// Get the line as a string
    pub fn get_line(&self) -> String {
        self.zleline.iter().collect()
    }

    /// Set the line from a string
    pub fn set_line(&mut self, s: &str) {
        self.zleline = s.chars().collect();
        self.zlell = self.zleline.len();
        self.zlecs = self.zlecs.min(self.zlell);
        self.resetneeded = true;
    }

    /// Clear the line
    pub fn clear_line(&mut self) {
        self.zleline.clear();
        self.zlell = 0;
        self.zlecs = 0;
        self.mark = 0;
        self.resetneeded = true;
    }

    /// Get region between point and mark
    pub fn get_region(&self) -> &[ZleChar] {
        let (start, end) = if self.zlecs < self.mark {
            (self.zlecs, self.mark)
        } else {
            (self.mark, self.zlecs)
        };
        &self.zleline[start..end]
    }

    /// Cut to named buffer
    pub fn cut_to_buffer(&mut self, buf: usize, append: bool) {
        if buf < self.vibuf.len() {
            let (start, end) = if self.zlecs < self.mark {
                (self.zlecs, self.mark)
            } else {
                (self.mark, self.zlecs)
            };

            let text: ZleString = self.zleline[start..end].to_vec();

            if append {
                self.vibuf[buf].extend(text);
            } else {
                self.vibuf[buf] = text;
            }
        }
    }

    /// Paste from named buffer
    pub fn paste_from_buffer(&mut self, buf: usize, after: bool) {
        if buf < self.vibuf.len() {
            let text = self.vibuf[buf].clone();
            if !text.is_empty() {
                if after && self.zlecs < self.zlell {
                    self.zlecs += 1;
                }
                self.insert_chars(&text);
            }
        }
    }
}

/// Metafication helpers (for compatibility with zsh's metafied strings)
pub fn metafy(s: &str) -> String {
    // In zsh, Meta (0x83) is used to escape special bytes
    // For Rust we typically don't need this, but provide for compatibility
    s.to_string()
}

pub fn unmetafy(s: &str) -> String {
    s.to_string()
}

/// String width calculation
pub fn strwidth(s: &str) -> usize {
    // TODO: use unicode-width for proper width calculation
    s.chars().count()
}

/// Check if character is printable
pub fn is_printable(c: char) -> bool {
    !c.is_control() && c != '\x7f'
}

/// Escape special characters for display
pub fn escape_for_display(c: char) -> String {
    if c.is_control() {
        if c as u32 <= 26 {
            format!("^{}", (c as u8 + b'@') as char)
        } else {
            format!("\\x{:02x}", c as u32)
        }
    } else if c == '\x7f' {
        "^?".to_string()
    } else {
        c.to_string()
    }
}
