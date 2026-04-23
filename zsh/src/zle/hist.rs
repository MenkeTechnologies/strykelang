//! ZLE history operations
//!
//! Direct port from zsh/Src/Zle/zle_hist.c

use super::main::{Zle, ZleString};

/// History entry
#[derive(Debug, Clone)]
pub struct HistEntry {
    /// The command line
    pub line: String,
    /// Event number
    pub num: i64,
    /// Timestamp (if available)
    pub time: Option<i64>,
}

/// History state
#[derive(Debug, Default)]
pub struct History {
    /// History entries (newest last)
    pub entries: Vec<HistEntry>,
    /// Current position in history
    pub cursor: usize,
    /// Maximum history size
    pub max_size: usize,
    /// Saved line when navigating history
    pub saved_line: Option<ZleString>,
    /// Saved cursor position
    pub saved_cs: usize,
    /// Search pattern
    pub search_pattern: String,
    /// Last search direction (true = backward)
    pub search_backward: bool,
}

impl History {
    pub fn new(max_size: usize) -> Self {
        History {
            entries: Vec::new(),
            cursor: 0,
            max_size,
            saved_line: None,
            saved_cs: 0,
            search_pattern: String::new(),
            search_backward: true,
        }
    }

    /// Add entry to history
    pub fn add(&mut self, line: String) {
        // Don't add empty or duplicate entries
        if line.is_empty() {
            return;
        }
        if let Some(last) = self.entries.last() {
            if last.line == line {
                return;
            }
        }

        self.entries.push(HistEntry {
            line,
            num: self.entries.len() as i64 + 1,
            time: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            ),
        });

        // Trim if over max size
        while self.entries.len() > self.max_size {
            self.entries.remove(0);
        }

        // Reset cursor to end
        self.cursor = self.entries.len();
    }

    /// Get entry at cursor
    pub fn get(&self, index: usize) -> Option<&HistEntry> {
        self.entries.get(index)
    }

    /// Move cursor up (older)
    pub fn up(&mut self) -> Option<&HistEntry> {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.entries.get(self.cursor)
        } else {
            None
        }
    }

    /// Move cursor down (newer)
    pub fn down(&mut self) -> Option<&HistEntry> {
        if self.cursor < self.entries.len() {
            self.cursor += 1;
            self.entries.get(self.cursor)
        } else {
            None
        }
    }

    /// Search backward for pattern
    pub fn search_backward(&mut self, pattern: &str) -> Option<&HistEntry> {
        let start = if self.cursor > 0 {
            self.cursor - 1
        } else {
            return None;
        };

        for i in (0..=start).rev() {
            if self.entries[i].line.contains(pattern) {
                self.cursor = i;
                return self.entries.get(i);
            }
        }

        None
    }

    /// Search forward for pattern
    pub fn search_forward(&mut self, pattern: &str) -> Option<&HistEntry> {
        for i in (self.cursor + 1)..self.entries.len() {
            if self.entries[i].line.contains(pattern) {
                self.cursor = i;
                return self.entries.get(i);
            }
        }

        None
    }

    /// Reset cursor to end
    pub fn reset(&mut self) {
        self.cursor = self.entries.len();
        self.saved_line = None;
    }
}

impl Zle {
    /// Initialize history for ZLE
    pub fn init_history(&mut self, max_size: usize) {
        // History would be stored externally and passed in
        // This is just a stub for the interface
        let _ = max_size;
    }

    /// Go to previous history entry
    pub fn history_up(&mut self, hist: &mut History) {
        if hist.saved_line.is_none() {
            // Save current line
            hist.saved_line = Some(self.zleline.clone());
            hist.saved_cs = self.zlecs;
        }

        if let Some(entry) = hist.up() {
            self.zleline = entry.line.chars().collect();
            self.zlell = self.zleline.len();
            self.zlecs = self.zlell;
            self.resetneeded = true;
        }
    }

    /// Go to next history entry
    pub fn history_down(&mut self, hist: &mut History) {
        if let Some(entry) = hist.down() {
            self.zleline = entry.line.chars().collect();
            self.zlell = self.zleline.len();
            self.zlecs = self.zlell;
            self.resetneeded = true;
        } else if let Some(saved) = hist.saved_line.take() {
            // Restore saved line
            self.zleline = saved;
            self.zlell = self.zleline.len();
            self.zlecs = hist.saved_cs;
            self.resetneeded = true;
        }
    }

    /// Incremental search backward
    pub fn history_isearch_backward(&mut self, hist: &mut History) {
        hist.search_backward = true;
        // TODO: implement full incremental search UI
    }

    /// Incremental search forward
    pub fn history_isearch_forward(&mut self, hist: &mut History) {
        hist.search_backward = false;
        // TODO: implement full incremental search UI
    }

    /// Search history for prefix
    pub fn history_search_prefix(&mut self, hist: &mut History) {
        let prefix: String = self.zleline[..self.zlecs].iter().collect();
        
        if let Some(entry) = hist.search_backward(&prefix) {
            self.zleline = entry.line.chars().collect();
            self.zlell = self.zleline.len();
            self.resetneeded = true;
        }
    }
}
