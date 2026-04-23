//! History management for zshrs
//!
//! Port from zsh/Src/hist.c
//!
//! Provides history expansion, history file management, and history ring.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// History entry
#[derive(Clone, Debug)]
pub struct HistEntry {
    pub histnum: i64,           // History event number
    pub text: String,           // Command text
    pub words: Vec<(usize, usize)>, // Word boundaries
    pub stim: i64,              // Start time
    pub ftim: i64,              // Finish time
    pub flags: u32,             // Entry flags
}

/// History entry flags
pub mod hist_flags {
    pub const OLD: u32 = 1;        // From history file
    pub const DUP: u32 = 2;        // Duplicate
    pub const FOREIGN: u32 = 4;    // From other session
    pub const TMPSTORE: u32 = 8;   // Temporary storage
    pub const NOWRITE: u32 = 16;   // Don't save to file
}

impl HistEntry {
    pub fn new(histnum: i64, text: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        HistEntry {
            histnum,
            text,
            words: Vec::new(),
            stim: now,
            ftim: now,
            flags: 0,
        }
    }

    /// Get a specific word from the entry
    pub fn get_word(&self, index: usize) -> Option<&str> {
        self.words.get(index).map(|(start, end)| &self.text[*start..*end])
    }

    /// Get number of words
    pub fn num_words(&self) -> usize {
        self.words.len()
    }
}

/// History active bits
pub const HA_ACTIVE: u32 = 1;     // History mechanism is active
pub const HA_NOINC: u32 = 2;      // Don't store, curhist not incremented
pub const HA_INWORD: u32 = 4;     // We're inside a word

/// History state
pub struct History {
    /// History entries indexed by event number
    entries: HashMap<i64, HistEntry>,
    /// Ring buffer order (newest first)
    ring: Vec<i64>,
    /// Current history number
    pub curhist: i64,
    /// History line count
    pub histlinect: i64,
    /// History size limit
    pub histsiz: i64,
    /// Save history size
    pub savehistsiz: i64,
    /// History active state
    pub histactive: u32,
    /// Stop history flag
    pub stophist: i32,
    /// History done flags
    pub histdone: i32,
    /// History skip flags
    pub hist_skip_flags: i32,
    /// Ignore all dups
    pub hist_ignore_all_dups: bool,
    /// Current line being edited
    pub curline: Option<HistEntry>,
    /// History substitution patterns
    pub hsubl: Option<String>,
    pub hsubr: Option<String>,
    /// Bang character
    pub bangchar: char,
    /// History file path
    pub histfile: Option<String>,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        History {
            entries: HashMap::new(),
            ring: Vec::new(),
            curhist: 0,
            histlinect: 0,
            histsiz: 1000,
            savehistsiz: 1000,
            histactive: 0,
            stophist: 0,
            histdone: 0,
            hist_skip_flags: 0,
            hist_ignore_all_dups: false,
            curline: None,
            hsubl: None,
            hsubr: None,
            bangchar: '!',
            histfile: None,
        }
    }

    /// Initialize history
    pub fn init(&mut self) {
        self.curhist = 0;
        self.histlinect = 0;
    }

    /// Begin history for a new command
    pub fn hbegin(&mut self, interactive: bool) {
        if (self.histactive & HA_ACTIVE) != 0 {
            return;
        }

        self.histactive = HA_ACTIVE;
        self.histdone = 0;

        if interactive {
            self.curhist += 1;
            self.curline = Some(HistEntry::new(self.curhist, String::new()));
        }
    }

    /// End history for current command
    pub fn hend(&mut self, text: Option<String>) -> bool {
        if (self.histactive & HA_ACTIVE) == 0 {
            return false;
        }

        self.histactive = 0;

        if let Some(mut entry) = self.curline.take() {
            if let Some(t) = text {
                entry.text = t;
            }

            // Skip empty entries
            if entry.text.trim().is_empty() {
                self.curhist -= 1;
                return false;
            }

            // Check for duplicates
            if self.hist_ignore_all_dups {
                let dup = self.entries.values().any(|e| e.text == entry.text);
                if dup {
                    self.curhist -= 1;
                    return false;
                }
            }

            // Add to history
            self.add_entry(entry);
            return true;
        }

        false
    }

    /// Add an entry to history
    fn add_entry(&mut self, entry: HistEntry) {
        let num = entry.histnum;
        
        // Remove old entry if at capacity
        while self.histlinect >= self.histsiz && !self.ring.is_empty() {
            let oldest = self.ring.pop().unwrap();
            self.entries.remove(&oldest);
            self.histlinect -= 1;
        }

        self.entries.insert(num, entry);
        self.ring.insert(0, num);
        self.histlinect += 1;
    }

    /// Get entry by history number
    pub fn get(&self, num: i64) -> Option<&HistEntry> {
        self.entries.get(&num)
    }

    /// Get the most recent entry
    pub fn latest(&self) -> Option<&HistEntry> {
        self.ring.first().and_then(|n| self.entries.get(n))
    }

    /// Get the n-th most recent entry (0 = latest)
    pub fn recent(&self, n: usize) -> Option<&HistEntry> {
        self.ring.get(n).and_then(|num| self.entries.get(num))
    }

    /// Search history backwards for a pattern
    pub fn search_back(&self, pattern: &str, start: i64) -> Option<&HistEntry> {
        for num in self.ring.iter() {
            if *num >= start {
                continue;
            }
            if let Some(entry) = self.entries.get(num) {
                if entry.text.contains(pattern) {
                    return Some(entry);
                }
            }
        }
        None
    }

    /// Search history forwards for a pattern
    pub fn search_forward(&self, pattern: &str, start: i64) -> Option<&HistEntry> {
        for num in self.ring.iter().rev() {
            if *num <= start {
                continue;
            }
            if let Some(entry) = self.entries.get(num) {
                if entry.text.contains(pattern) {
                    return Some(entry);
                }
            }
        }
        None
    }

    /// Perform history substitution
    pub fn expand(&mut self, line: &str) -> Result<String, String> {
        let mut result = String::new();
        let mut chars = line.chars().peekable();
        let bang = self.bangchar;

        while let Some(c) = chars.next() {
            if c == bang {
                match chars.peek() {
                    Some(&'!') => {
                        // !! - last command
                        chars.next();
                        if let Some(entry) = self.latest() {
                            result.push_str(&entry.text);
                        } else {
                            return Err("No previous command".to_string());
                        }
                    }
                    Some(&'-') | Some(&('0'..='9')) => {
                        // !n or !-n
                        let mut numstr = String::new();
                        if chars.peek() == Some(&'-') {
                            numstr.push(chars.next().unwrap());
                        }
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() {
                                numstr.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                        if let Ok(n) = numstr.parse::<i64>() {
                            let target = if n < 0 {
                                self.curhist + n
                            } else {
                                n
                            };
                            if let Some(entry) = self.get(target) {
                                result.push_str(&entry.text);
                            } else {
                                return Err(format!("!{}: event not found", numstr));
                            }
                        }
                    }
                    Some(&'?') => {
                        // !?string - search
                        chars.next();
                        let mut pattern = String::new();
                        while let Some(&c) = chars.peek() {
                            if c == '?' {
                                chars.next();
                                break;
                            }
                            pattern.push(chars.next().unwrap());
                        }
                        if let Some(entry) = self.search_back(&pattern, self.curhist) {
                            result.push_str(&entry.text);
                        } else {
                            return Err(format!("!?{}: event not found", pattern));
                        }
                    }
                    Some(&'^') | Some(&'$') | Some(&'*') | Some(&':') => {
                        // Word designators on last command
                        if let Some(entry) = self.latest() {
                            let words: Vec<&str> = entry.text.split_whitespace().collect();
                            match chars.next() {
                                Some('^') => {
                                    if let Some(w) = words.get(1) {
                                        result.push_str(w);
                                    }
                                }
                                Some('$') => {
                                    if let Some(w) = words.last() {
                                        result.push_str(w);
                                    }
                                }
                                Some('*') => {
                                    result.push_str(&words[1..].join(" "));
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(c) if c.is_alphabetic() => {
                        // !string - search prefix
                        let mut pattern = String::new();
                        while let Some(&c) = chars.peek() {
                            if c.is_alphanumeric() || c == '_' {
                                pattern.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                        let found = self.ring.iter().find_map(|num| {
                            self.entries.get(num).filter(|e| e.text.starts_with(&pattern))
                        });
                        if let Some(entry) = found {
                            result.push_str(&entry.text);
                        } else {
                            return Err(format!("!{}: event not found", pattern));
                        }
                    }
                    _ => result.push(bang),
                }
            } else if c == '^' && result.is_empty() {
                // ^old^new - quick substitution
                let mut old = String::new();
                let mut new = String::new();
                let mut in_new = false;

                while let Some(c) = chars.next() {
                    if c == '^' {
                        if in_new {
                            break;
                        }
                        in_new = true;
                    } else if in_new {
                        new.push(c);
                    } else {
                        old.push(c);
                    }
                }

                if let Some(entry) = self.latest() {
                    result = entry.text.replacen(&old, &new, 1);
                    self.hsubl = Some(old);
                    self.hsubr = Some(new);
                } else {
                    return Err("No previous command".to_string());
                }
            } else {
                result.push(c);
            }
        }

        Ok(result)
    }

    /// Read history file
    pub fn read_file(&mut self, path: &Path) -> io::Result<()> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            
            // Parse extended history format
            if line.starts_with(':') {
                // Extended format: : timestamp:0;command
                let parts: Vec<&str> = line.splitn(2, ';').collect();
                if parts.len() == 2 {
                    let text = parts[1].to_string();
                    let mut entry = HistEntry::new(self.curhist + 1, text);
                    
                    // Parse timestamp
                    if let Some(ts_part) = parts[0].strip_prefix(": ") {
                        if let Some(ts_str) = ts_part.split(':').next() {
                            if let Ok(ts) = ts_str.parse::<i64>() {
                                entry.stim = ts;
                                entry.ftim = ts;
                            }
                        }
                    }
                    
                    entry.flags |= hist_flags::OLD;
                    self.curhist += 1;
                    self.add_entry(entry);
                }
            } else if !line.is_empty() {
                // Simple format
                self.curhist += 1;
                let mut entry = HistEntry::new(self.curhist, line);
                entry.flags |= hist_flags::OLD;
                self.add_entry(entry);
            }
        }

        Ok(())
    }

    /// Write history file
    pub fn write_file(&self, path: &Path, append: bool) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(!append)
            .append(append)
            .open(path)?;

        for num in self.ring.iter().rev() {
            if let Some(entry) = self.entries.get(num) {
                if (entry.flags & hist_flags::NOWRITE) != 0 {
                    continue;
                }
                // Write extended format
                writeln!(file, ": {}:0;{}", entry.stim, entry.text)?;
            }
        }

        Ok(())
    }

    /// Clear all history
    pub fn clear(&mut self) {
        self.entries.clear();
        self.ring.clear();
        self.histlinect = 0;
    }

    /// Get all entries in order
    pub fn all_entries(&self) -> Vec<&HistEntry> {
        self.ring.iter().filter_map(|n| self.entries.get(n)).collect()
    }

    /// Number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Save history context
#[derive(Clone)]
pub struct HistStack {
    pub histactive: u32,
    pub histdone: i32,
    pub stophist: i32,
    pub chline: Option<String>,
    pub hptr: usize,
    pub chwords: Vec<(usize, usize)>,
    pub hlinesz: usize,
    pub defev: i64,
}

impl Default for HistStack {
    fn default() -> Self {
        HistStack {
            histactive: 0,
            histdone: 0,
            stophist: 0,
            chline: None,
            hptr: 0,
            chwords: Vec::new(),
            hlinesz: 0,
            defev: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_add() {
        let mut hist = History::new();
        hist.hbegin(true);
        hist.hend(Some("echo hello".to_string()));

        assert_eq!(hist.len(), 1);
        assert_eq!(hist.latest().unwrap().text, "echo hello");
    }

    #[test]
    fn test_history_expand_bang_bang() {
        let mut hist = History::new();
        hist.hbegin(true);
        hist.hend(Some("ls -la".to_string()));

        let result = hist.expand("!! | grep foo").unwrap();
        assert_eq!(result, "ls -la | grep foo");
    }

    #[test]
    fn test_history_expand_caret() {
        let mut hist = History::new();
        hist.hbegin(true);
        hist.hend(Some("echo hello".to_string()));

        let result = hist.expand("^hello^world").unwrap();
        assert_eq!(result, "echo world");
    }

    #[test]
    fn test_history_search() {
        let mut hist = History::new();
        
        hist.hbegin(true);
        hist.hend(Some("cd /tmp".to_string()));
        
        hist.hbegin(true);
        hist.hend(Some("echo hello".to_string()));
        
        hist.hbegin(true);
        hist.hend(Some("ls -la".to_string()));

        let result = hist.search_back("echo", hist.curhist + 1);
        assert!(result.is_some());
        assert_eq!(result.unwrap().text, "echo hello");
    }

    #[test]
    fn test_history_capacity() {
        let mut hist = History::new();
        hist.histsiz = 3;

        for i in 0..5 {
            hist.hbegin(true);
            hist.hend(Some(format!("cmd{}", i)));
        }

        assert_eq!(hist.len(), 3);
        assert!(hist.get(1).is_none());
        assert!(hist.get(2).is_none());
    }
}
