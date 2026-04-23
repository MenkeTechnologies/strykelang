//! ZLE keymap and key bindings - Direct port from zsh/Src/Zle/zle_keymap.c
//!
//! Keymap structures:
//!
//! There is a hash table of keymap names. Each name just points to a keymap.
//! More than one name may point to the same keymap.
//!
//! Each keymap consists of a table of bindings for each character, and a
//! hash table of multi-character key bindings. The keymap has no individual
//! name, but maintains a reference count.

use std::collections::HashMap;
use std::sync::Arc;

use super::thingy::Thingy;

/// Flags for keymap names
#[derive(Debug, Clone, Copy, Default)]
pub struct KeymapNameFlags {
    /// Can't be deleted (.safe)
    pub immortal: bool,
}

/// A named reference to a keymap
#[derive(Debug, Clone)]
pub struct KeymapName {
    pub name: String,
    pub flags: KeymapNameFlags,
    pub keymap: Arc<Keymap>,
}

/// Flags for keymaps
#[derive(Debug, Clone, Copy, Default)]
pub struct KeymapFlags {
    /// Keymap is immutable
    pub immutable: bool,
}

/// A keymap - binding of keys to thingies
#[derive(Debug, Clone)]
pub struct Keymap {
    /// Base binding of each character (0-255)
    pub first: [Option<Thingy>; 256],
    /// Multi-character bindings (key sequence -> binding)
    pub multi: HashMap<Vec<u8>, KeyBinding>,
    /// Primary name of this keymap
    pub primary: Option<String>,
    /// Flags
    pub flags: KeymapFlags,
}

/// A key binding (either a thingy or a string to send)
#[derive(Debug, Clone)]
pub struct KeyBinding {
    /// The thingy this key is bound to (None for send-string)
    pub bind: Option<Thingy>,
    /// String to send (metafied)
    pub str: Option<String>,
    /// Number of sequences for which this is a prefix
    pub prefixct: i32,
}

/// State for listing keymaps
#[derive(Debug, Clone, Default)]
pub struct BindState {
    pub flags: BindStateFlags,
    pub kmname: String,
    pub firstseq: Vec<u8>,
    pub lastseq: Vec<u8>,
    pub bind: Option<Thingy>,
    pub str: Option<String>,
    pub prefix: Vec<u8>,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct BindStateFlags: u32 {
        const LIST = 1 << 0;
        const ALL = 1 << 1;
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap {
            first: std::array::from_fn(|_| None),
            multi: HashMap::new(),
            primary: None,
            flags: KeymapFlags::default(),
        }
    }
}

impl Keymap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a single character to a thingy
    pub fn bind_char(&mut self, c: u8, thingy: Thingy) {
        self.first[c as usize] = Some(thingy);
    }

    /// Unbind a single character
    pub fn unbind_char(&mut self, c: u8) {
        self.first[c as usize] = None;
    }

    /// Bind a key sequence to a thingy
    pub fn bind_seq(&mut self, seq: &[u8], thingy: Thingy) {
        if seq.len() == 1 {
            self.bind_char(seq[0], thingy);
        } else {
            // Mark prefixes
            for i in 1..seq.len() {
                let prefix = &seq[..i];
                self.multi
                    .entry(prefix.to_vec())
                    .and_modify(|kb| kb.prefixct += 1)
                    .or_insert(KeyBinding {
                        bind: None,
                        str: None,
                        prefixct: 1,
                    });
            }

            // Add the binding
            self.multi.insert(
                seq.to_vec(),
                KeyBinding {
                    bind: Some(thingy),
                    str: None,
                    prefixct: 0,
                },
            );
        }
    }

    /// Bind a key sequence to a string (send-string)
    pub fn bind_str(&mut self, seq: &[u8], s: String) {
        if seq.len() == 1 {
            // Single char can't be send-string in first[] table
            // Store in multi
        }

        // Mark prefixes
        for i in 1..seq.len() {
            let prefix = &seq[..i];
            self.multi
                .entry(prefix.to_vec())
                .and_modify(|kb| kb.prefixct += 1)
                .or_insert(KeyBinding {
                    bind: None,
                    str: None,
                    prefixct: 1,
                });
        }

        self.multi.insert(
            seq.to_vec(),
            KeyBinding {
                bind: None,
                str: Some(s),
                prefixct: 0,
            },
        );
    }

    /// Unbind a key sequence
    pub fn unbind_seq(&mut self, seq: &[u8]) {
        if seq.len() == 1 {
            self.unbind_char(seq[0]);
        } else {
            if self.multi.remove(seq).is_some() {
                // Decrement prefix counts
                for i in 1..seq.len() {
                    let prefix = &seq[..i];
                    if let Some(kb) = self.multi.get_mut(prefix) {
                        kb.prefixct -= 1;
                        if kb.prefixct == 0 && kb.bind.is_none() && kb.str.is_none() {
                            // Remove empty prefix entry
                            // (can't remove while iterating, so we'll leave it)
                        }
                    }
                }
            }
        }
    }

    /// Look up a single character binding
    pub fn lookup_char(&self, c: u8) -> Option<&Thingy> {
        self.first[c as usize].as_ref()
    }

    /// Look up a key sequence binding (for multi-char sequences only)
    pub fn lookup_seq(&self, seq: &[u8]) -> Option<&KeyBinding> {
        if seq.len() == 1 {
            // For single char, use lookup_char instead
            None
        } else {
            self.multi.get(seq)
        }
    }

    /// Check if a sequence is a prefix
    pub fn is_prefix(&self, seq: &[u8]) -> bool {
        if seq.len() == 1 {
            // Check if this char is a prefix in multi table
            self.multi.keys().any(|k| k.len() > 1 && k[0] == seq[0])
        } else {
            self.multi
                .get(seq)
                .map(|kb| kb.prefixct > 0)
                .unwrap_or(false)
        }
    }
}

/// Manager for all keymaps
#[derive(Debug)]
pub struct KeymapManager {
    /// Named keymaps
    pub keymaps: HashMap<String, Arc<Keymap>>,
    /// Current keymap
    pub current: Option<Arc<Keymap>>,
    /// Current keymap name
    pub current_name: String,
    /// Local keymap (temporary override)
    pub local: Option<Arc<Keymap>>,
    /// Key sequence buffer
    pub keybuf: Vec<u8>,
    /// Last named command executed
    pub lastnamed: Option<Thingy>,
}

impl Default for KeymapManager {
    fn default() -> Self {
        Self::new()
    }
}

impl KeymapManager {
    pub fn new() -> Self {
        let mut mgr = KeymapManager {
            keymaps: HashMap::new(),
            current: None,
            current_name: "main".to_string(),
            local: None,
            keybuf: Vec::with_capacity(20),
            lastnamed: None,
        };

        // Create default keymaps
        mgr.create_default_keymaps();

        mgr
    }

    /// Create the default keymaps (emacs, viins, vicmd, etc.)
    fn create_default_keymaps(&mut self) {
        // Create emacs keymap
        let mut emacs = Keymap::new();
        emacs.primary = Some("emacs".to_string());
        self.setup_emacs_keymap(&mut emacs);
        self.keymaps.insert("emacs".to_string(), Arc::new(emacs));

        // Create viins keymap
        let mut viins = Keymap::new();
        viins.primary = Some("viins".to_string());
        self.setup_viins_keymap(&mut viins);
        self.keymaps.insert("viins".to_string(), Arc::new(viins));

        // Create vicmd keymap
        let mut vicmd = Keymap::new();
        vicmd.primary = Some("vicmd".to_string());
        self.setup_vicmd_keymap(&mut vicmd);
        self.keymaps.insert("vicmd".to_string(), Arc::new(vicmd));

        // Create isearch keymap
        let isearch = Keymap::new();
        self.keymaps.insert("isearch".to_string(), Arc::new(isearch));

        // Create command keymap
        let command = Keymap::new();
        self.keymaps.insert("command".to_string(), Arc::new(command));

        // "main" is initially aliased to emacs
        let emacs = self.keymaps.get("emacs").cloned();
        if let Some(emacs) = emacs {
            self.keymaps.insert("main".to_string(), Arc::clone(&emacs));
            self.current = Some(emacs);
        }
    }

    /// Set up emacs keymap bindings
    fn setup_emacs_keymap(&self, km: &mut Keymap) {
        // Self-insert for printable characters
        for c in 32u8..127 {
            km.bind_char(c, Thingy::builtin("self-insert"));
        }

        // Basic movement
        km.bind_char(0x01, Thingy::builtin("beginning-of-line")); // Ctrl-A
        km.bind_char(0x02, Thingy::builtin("backward-char")); // Ctrl-B
        km.bind_char(0x04, Thingy::builtin("delete-char-or-list")); // Ctrl-D
        km.bind_char(0x05, Thingy::builtin("end-of-line")); // Ctrl-E
        km.bind_char(0x06, Thingy::builtin("forward-char")); // Ctrl-F

        // Editing
        km.bind_char(0x08, Thingy::builtin("backward-delete-char")); // Ctrl-H / Backspace
        km.bind_char(0x0B, Thingy::builtin("kill-line")); // Ctrl-K
        km.bind_char(0x0C, Thingy::builtin("clear-screen")); // Ctrl-L
        km.bind_char(0x0D, Thingy::builtin("accept-line")); // Ctrl-M / Enter
        km.bind_char(0x0E, Thingy::builtin("down-line-or-history")); // Ctrl-N
        km.bind_char(0x10, Thingy::builtin("up-line-or-history")); // Ctrl-P
        km.bind_char(0x12, Thingy::builtin("history-incremental-search-backward")); // Ctrl-R
        km.bind_char(0x13, Thingy::builtin("history-incremental-search-forward")); // Ctrl-S
        km.bind_char(0x14, Thingy::builtin("transpose-chars")); // Ctrl-T
        km.bind_char(0x15, Thingy::builtin("kill-whole-line")); // Ctrl-U
        km.bind_char(0x17, Thingy::builtin("backward-kill-word")); // Ctrl-W
        km.bind_char(0x19, Thingy::builtin("yank")); // Ctrl-Y

        // Ctrl-C (interrupt) - mapped to send-break
        km.bind_char(0x03, Thingy::builtin("send-break"));

        // Tab completion
        km.bind_char(0x09, Thingy::builtin("expand-or-complete")); // Tab

        // Delete/Backspace
        km.bind_char(0x7F, Thingy::builtin("backward-delete-char")); // DEL

        // Escape sequences would go in multi-char bindings
        // ESC + char sequences
        km.bind_seq(b"\x1bb", Thingy::builtin("backward-word")); // Alt-B
        km.bind_seq(b"\x1bf", Thingy::builtin("forward-word")); // Alt-F
        km.bind_seq(b"\x1bd", Thingy::builtin("kill-word")); // Alt-D
        km.bind_seq(b"\x1b\x7f", Thingy::builtin("backward-kill-word")); // Alt-Backspace

        // Arrow keys (common ANSI sequences)
        km.bind_seq(b"\x1b[A", Thingy::builtin("up-line-or-history")); // Up
        km.bind_seq(b"\x1b[B", Thingy::builtin("down-line-or-history")); // Down
        km.bind_seq(b"\x1b[C", Thingy::builtin("forward-char")); // Right
        km.bind_seq(b"\x1b[D", Thingy::builtin("backward-char")); // Left
        km.bind_seq(b"\x1b[H", Thingy::builtin("beginning-of-line")); // Home
        km.bind_seq(b"\x1b[F", Thingy::builtin("end-of-line")); // End
        km.bind_seq(b"\x1b[3~", Thingy::builtin("delete-char")); // Delete

        // Alternative arrow key sequences
        km.bind_seq(b"\x1bOA", Thingy::builtin("up-line-or-history"));
        km.bind_seq(b"\x1bOB", Thingy::builtin("down-line-or-history"));
        km.bind_seq(b"\x1bOC", Thingy::builtin("forward-char"));
        km.bind_seq(b"\x1bOD", Thingy::builtin("backward-char"));
    }

    /// Set up viins (vi insert mode) keymap bindings
    fn setup_viins_keymap(&self, km: &mut Keymap) {
        // Self-insert for printable characters
        for c in 32u8..127 {
            km.bind_char(c, Thingy::builtin("self-insert"));
        }

        // Escape to command mode
        km.bind_char(0x1B, Thingy::builtin("vi-cmd-mode")); // ESC

        // Basic editing
        km.bind_char(0x08, Thingy::builtin("vi-backward-delete-char")); // Ctrl-H
        km.bind_char(0x7F, Thingy::builtin("vi-backward-delete-char")); // DEL
        km.bind_char(0x0D, Thingy::builtin("accept-line")); // Enter
        km.bind_char(0x09, Thingy::builtin("expand-or-complete")); // Tab

        // Ctrl-C
        km.bind_char(0x03, Thingy::builtin("send-break"));

        // Ctrl-W
        km.bind_char(0x17, Thingy::builtin("vi-backward-kill-word"));
    }

    /// Set up vicmd (vi command mode) keymap bindings
    fn setup_vicmd_keymap(&self, km: &mut Keymap) {
        // Movement
        km.bind_char(b'h', Thingy::builtin("vi-backward-char"));
        km.bind_char(b'l', Thingy::builtin("vi-forward-char"));
        km.bind_char(b'j', Thingy::builtin("down-line-or-history"));
        km.bind_char(b'k', Thingy::builtin("up-line-or-history"));
        km.bind_char(b'w', Thingy::builtin("vi-forward-word"));
        km.bind_char(b'W', Thingy::builtin("vi-forward-blank-word"));
        km.bind_char(b'b', Thingy::builtin("vi-backward-word"));
        km.bind_char(b'B', Thingy::builtin("vi-backward-blank-word"));
        km.bind_char(b'e', Thingy::builtin("vi-forward-word-end"));
        km.bind_char(b'E', Thingy::builtin("vi-forward-blank-word-end"));
        km.bind_char(b'0', Thingy::builtin("vi-digit-or-beginning-of-line"));
        km.bind_char(b'^', Thingy::builtin("vi-first-non-blank"));
        km.bind_char(b'$', Thingy::builtin("vi-end-of-line"));

        // Mode switching
        km.bind_char(b'i', Thingy::builtin("vi-insert"));
        km.bind_char(b'I', Thingy::builtin("vi-insert-bol"));
        km.bind_char(b'a', Thingy::builtin("vi-add-next"));
        km.bind_char(b'A', Thingy::builtin("vi-add-eol"));
        km.bind_char(b'o', Thingy::builtin("vi-open-line-below"));
        km.bind_char(b'O', Thingy::builtin("vi-open-line-above"));

        // Editing
        km.bind_char(b'x', Thingy::builtin("vi-delete-char"));
        km.bind_char(b'X', Thingy::builtin("vi-backward-delete-char"));
        km.bind_char(b'd', Thingy::builtin("vi-delete"));
        km.bind_char(b'D', Thingy::builtin("vi-kill-eol"));
        km.bind_char(b'c', Thingy::builtin("vi-change"));
        km.bind_char(b'C', Thingy::builtin("vi-change-eol"));
        km.bind_char(b'y', Thingy::builtin("vi-yank"));
        km.bind_char(b'Y', Thingy::builtin("vi-yank-whole-line"));
        km.bind_char(b'p', Thingy::builtin("vi-put-after"));
        km.bind_char(b'P', Thingy::builtin("vi-put-before"));
        km.bind_char(b'r', Thingy::builtin("vi-replace-chars"));
        km.bind_char(b'R', Thingy::builtin("vi-replace"));
        km.bind_char(b's', Thingy::builtin("vi-substitute"));
        km.bind_char(b'S', Thingy::builtin("vi-change-whole-line"));

        // Search
        km.bind_char(b'/', Thingy::builtin("vi-history-search-forward"));
        km.bind_char(b'?', Thingy::builtin("vi-history-search-backward"));
        km.bind_char(b'n', Thingy::builtin("vi-repeat-search"));
        km.bind_char(b'N', Thingy::builtin("vi-rev-repeat-search"));
        km.bind_char(b'f', Thingy::builtin("vi-find-next-char"));
        km.bind_char(b'F', Thingy::builtin("vi-find-prev-char"));
        km.bind_char(b't', Thingy::builtin("vi-find-next-char-skip"));
        km.bind_char(b'T', Thingy::builtin("vi-find-prev-char-skip"));
        km.bind_char(b';', Thingy::builtin("vi-repeat-find"));
        km.bind_char(b',', Thingy::builtin("vi-rev-repeat-find"));

        // Undo
        km.bind_char(b'u', Thingy::builtin("undo"));
        km.bind_char(0x12, Thingy::builtin("redo")); // Ctrl-R

        // Repeat
        km.bind_char(b'.', Thingy::builtin("vi-repeat-change"));

        // Digit arguments
        for c in b'1'..=b'9' {
            km.bind_char(c, Thingy::builtin("digit-argument"));
        }

        // Accept line
        km.bind_char(0x0D, Thingy::builtin("accept-line"));

        // Ctrl-C
        km.bind_char(0x03, Thingy::builtin("send-break"));

        // Join lines
        km.bind_char(b'J', Thingy::builtin("vi-join"));

        // Goto
        km.bind_char(b'G', Thingy::builtin("vi-fetch-history"));
        km.bind_char(b'g', Thingy::builtin("vi-goto-column")); // Actually prefix, but simplified
    }

    /// Get a keymap by name
    pub fn get(&self, name: &str) -> Option<Arc<Keymap>> {
        self.keymaps.get(name).cloned()
    }

    /// Set the current keymap
    pub fn select(&mut self, name: &str) -> bool {
        if let Some(km) = self.keymaps.get(name) {
            self.current = Some(Arc::clone(km));
            self.current_name = name.to_string();
            true
        } else {
            false
        }
    }

    /// Link a new name to an existing keymap
    pub fn link(&mut self, oldname: &str, newname: &str) -> bool {
        if let Some(km) = self.keymaps.get(oldname) {
            self.keymaps.insert(newname.to_string(), Arc::clone(km));
            true
        } else {
            false
        }
    }

    /// Delete a keymap name
    pub fn delete(&mut self, name: &str) -> bool {
        // Don't allow deleting immortal keymaps
        if name == "main" || name == "emacs" || name == "viins" || name == "vicmd" {
            return false;
        }
        self.keymaps.remove(name).is_some()
    }

    /// Look up a key in the current keymap
    pub fn lookup_key(&self, c: char) -> Option<Thingy> {
        let km = self.local.as_ref().or(self.current.as_ref())?;

        // For now, just look up single byte
        if c as u32 <= 255 {
            km.first[c as usize].clone()
        } else {
            None
        }
    }

    /// Look up a key sequence in the current keymap
    pub fn lookup_seq(&self, seq: &[u8]) -> Option<&KeyBinding> {
        let km = self.local.as_ref().or(self.current.as_ref())?;
        km.lookup_seq(seq)
    }

    /// Check if a sequence is a prefix in the current keymap
    pub fn is_prefix(&self, seq: &[u8]) -> bool {
        if let Some(km) = self.local.as_ref().or(self.current.as_ref()) {
            km.is_prefix(seq)
        } else {
            false
        }
    }

    /// List all keymap names
    pub fn list_names(&self) -> Vec<&String> {
        self.keymaps.keys().collect()
    }
}
