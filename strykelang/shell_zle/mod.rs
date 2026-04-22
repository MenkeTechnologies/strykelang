//! ZLE (Zsh Line Editor) module
//!
//! This module provides zsh-compatible line editing functionality including:
//! - Widget system (ZLE functions)
//! - Key bindings (bindkey)
//! - Keymaps (emacs, viins, vicmd, etc.)
//! - User-defined widgets
//! - ZLE special parameters (BUFFER, CURSOR, LBUFFER, RBUFFER, etc.)

mod bindings;
mod keymaps;
mod widgets;

pub use bindings::{KeyBinding, KeySequence};
pub use keymaps::{Keymap, KeymapName};
pub use widgets::{BuiltinWidget, Widget, WidgetResult};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// ZLE state - the line editor context
#[derive(Debug, Clone)]
pub struct ZleState {
    /// The current line buffer
    pub buffer: String,
    /// Cursor position (0-indexed, in characters)
    pub cursor: usize,
    /// Mark position for region operations
    pub mark: usize,
    /// Current keymap name
    pub keymap: KeymapName,
    /// Pending keys (for multi-key sequences)
    pub pending_keys: Vec<char>,
    /// Last widget executed
    pub last_widget: Option<String>,
    /// Numeric argument (from digit-argument, etc.)
    pub numeric_arg: Option<i32>,
    /// Kill ring (for yank operations)
    pub kill_ring: Vec<String>,
    /// Kill ring index
    pub kill_ring_index: usize,
    /// Undo stack
    pub undo_stack: Vec<UndoEntry>,
    /// Redo stack
    pub redo_stack: Vec<UndoEntry>,
    /// Whether in vi command mode
    pub vi_cmd_mode: bool,
    /// Pre-display text (prompt continuation, etc.)
    pub predisplay: String,
    /// Post-display text
    pub postdisplay: String,
    /// Region active flag
    pub region_active: bool,
    /// History line number being edited
    pub histno: usize,
    /// Pending input (for read-command widget)
    pub pending_input: String,
}

#[derive(Debug, Clone)]
pub struct UndoEntry {
    pub buffer: String,
    pub cursor: usize,
}

impl Default for ZleState {
    fn default() -> Self {
        Self::new()
    }
}

impl ZleState {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            mark: 0,
            keymap: KeymapName::Emacs,
            pending_keys: Vec::new(),
            last_widget: None,
            numeric_arg: None,
            kill_ring: Vec::with_capacity(8),
            kill_ring_index: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            vi_cmd_mode: false,
            predisplay: String::new(),
            postdisplay: String::new(),
            region_active: false,
            histno: 0,
            pending_input: String::new(),
        }
    }

    /// Get text before cursor (LBUFFER)
    pub fn lbuffer(&self) -> &str {
        &self.buffer[..self.cursor.min(self.buffer.len())]
    }

    /// Get text after cursor (RBUFFER)
    pub fn rbuffer(&self) -> &str {
        &self.buffer[self.cursor.min(self.buffer.len())..]
    }

    /// Set text before cursor
    pub fn set_lbuffer(&mut self, s: &str) {
        let rbuffer = self.rbuffer().to_string();
        self.buffer = format!("{}{}", s, rbuffer);
        self.cursor = s.len();
    }

    /// Set text after cursor
    pub fn set_rbuffer(&mut self, s: &str) {
        let lbuffer = self.lbuffer().to_string();
        self.buffer = format!("{}{}", lbuffer, s);
    }

    /// Save current state for undo
    pub fn save_undo(&mut self) {
        self.undo_stack.push(UndoEntry {
            buffer: self.buffer.clone(),
            cursor: self.cursor,
        });
        self.redo_stack.clear();
    }

    /// Undo last change
    pub fn undo(&mut self) -> bool {
        if let Some(entry) = self.undo_stack.pop() {
            self.redo_stack.push(UndoEntry {
                buffer: self.buffer.clone(),
                cursor: self.cursor,
            });
            self.buffer = entry.buffer;
            self.cursor = entry.cursor;
            true
        } else {
            false
        }
    }

    /// Redo last undone change
    pub fn redo(&mut self) -> bool {
        if let Some(entry) = self.redo_stack.pop() {
            self.undo_stack.push(UndoEntry {
                buffer: self.buffer.clone(),
                cursor: self.cursor,
            });
            self.buffer = entry.buffer;
            self.cursor = entry.cursor;
            true
        } else {
            false
        }
    }

    /// Add to kill ring
    pub fn kill_add(&mut self, text: &str) {
        if self.kill_ring.len() >= 8 {
            self.kill_ring.remove(0);
        }
        self.kill_ring.push(text.to_string());
        self.kill_ring_index = self.kill_ring.len().saturating_sub(1);
    }

    /// Yank from kill ring
    pub fn yank(&self) -> Option<&str> {
        self.kill_ring.get(self.kill_ring_index).map(|s| s.as_str())
    }

    /// Rotate kill ring
    pub fn yank_pop(&mut self) -> Option<&str> {
        if self.kill_ring.is_empty() {
            return None;
        }
        self.kill_ring_index = if self.kill_ring_index == 0 {
            self.kill_ring.len() - 1
        } else {
            self.kill_ring_index - 1
        };
        self.kill_ring.get(self.kill_ring_index).map(|s| s.as_str())
    }
}

/// The ZLE engine
pub struct Zle {
    /// Current state
    pub state: ZleState,
    /// Registered widgets
    widgets: HashMap<String, Widget>,
    /// Keymaps
    pub keymaps: HashMap<KeymapName, Keymap>,
    /// User-defined widgets (shell functions)
    user_widgets: HashMap<String, String>,
}

impl Default for Zle {
    fn default() -> Self {
        Self::new()
    }
}

impl Zle {
    pub fn new() -> Self {
        let mut zle = Self {
            state: ZleState::new(),
            widgets: HashMap::new(),
            keymaps: HashMap::new(),
            user_widgets: HashMap::new(),
        };
        zle.register_builtin_widgets();
        zle.setup_default_keymaps();
        zle
    }

    /// Register all builtin widgets
    fn register_builtin_widgets(&mut self) {
        use widgets::*;

        // Movement widgets
        self.widgets.insert(
            "forward-char".into(),
            Widget::Builtin(BuiltinWidget::ForwardChar),
        );
        self.widgets.insert(
            "backward-char".into(),
            Widget::Builtin(BuiltinWidget::BackwardChar),
        );
        self.widgets.insert(
            "forward-word".into(),
            Widget::Builtin(BuiltinWidget::ForwardWord),
        );
        self.widgets.insert(
            "backward-word".into(),
            Widget::Builtin(BuiltinWidget::BackwardWord),
        );
        self.widgets.insert(
            "beginning-of-line".into(),
            Widget::Builtin(BuiltinWidget::BeginningOfLine),
        );
        self.widgets.insert(
            "end-of-line".into(),
            Widget::Builtin(BuiltinWidget::EndOfLine),
        );

        // Editing widgets
        self.widgets.insert(
            "self-insert".into(),
            Widget::Builtin(BuiltinWidget::SelfInsert),
        );
        self.widgets.insert(
            "delete-char".into(),
            Widget::Builtin(BuiltinWidget::DeleteChar),
        );
        self.widgets.insert(
            "backward-delete-char".into(),
            Widget::Builtin(BuiltinWidget::BackwardDeleteChar),
        );
        self.widgets
            .insert("kill-line".into(), Widget::Builtin(BuiltinWidget::KillLine));
        self.widgets.insert(
            "backward-kill-line".into(),
            Widget::Builtin(BuiltinWidget::BackwardKillLine),
        );
        self.widgets
            .insert("kill-word".into(), Widget::Builtin(BuiltinWidget::KillWord));
        self.widgets.insert(
            "backward-kill-word".into(),
            Widget::Builtin(BuiltinWidget::BackwardKillWord),
        );
        self.widgets.insert(
            "kill-whole-line".into(),
            Widget::Builtin(BuiltinWidget::KillWholeLine),
        );
        self.widgets
            .insert("yank".into(), Widget::Builtin(BuiltinWidget::Yank));
        self.widgets
            .insert("yank-pop".into(), Widget::Builtin(BuiltinWidget::YankPop));

        // Undo widgets
        self.widgets
            .insert("undo".into(), Widget::Builtin(BuiltinWidget::Undo));
        self.widgets
            .insert("redo".into(), Widget::Builtin(BuiltinWidget::Redo));

        // History widgets
        self.widgets.insert(
            "up-line-or-history".into(),
            Widget::Builtin(BuiltinWidget::UpLineOrHistory),
        );
        self.widgets.insert(
            "down-line-or-history".into(),
            Widget::Builtin(BuiltinWidget::DownLineOrHistory),
        );
        self.widgets.insert(
            "beginning-of-history".into(),
            Widget::Builtin(BuiltinWidget::BeginningOfHistory),
        );
        self.widgets.insert(
            "end-of-history".into(),
            Widget::Builtin(BuiltinWidget::EndOfHistory),
        );
        self.widgets.insert(
            "history-incremental-search-backward".into(),
            Widget::Builtin(BuiltinWidget::HistoryIncrementalSearchBackward),
        );
        self.widgets.insert(
            "history-incremental-search-forward".into(),
            Widget::Builtin(BuiltinWidget::HistoryIncrementalSearchForward),
        );

        // Completion widgets
        self.widgets.insert(
            "expand-or-complete".into(),
            Widget::Builtin(BuiltinWidget::ExpandOrComplete),
        );
        self.widgets.insert(
            "complete-word".into(),
            Widget::Builtin(BuiltinWidget::CompleteWord),
        );
        self.widgets.insert(
            "menu-complete".into(),
            Widget::Builtin(BuiltinWidget::MenuComplete),
        );
        self.widgets.insert(
            "reverse-menu-complete".into(),
            Widget::Builtin(BuiltinWidget::ReverseMenuComplete),
        );

        // Accept/execute widgets
        self.widgets.insert(
            "accept-line".into(),
            Widget::Builtin(BuiltinWidget::AcceptLine),
        );
        self.widgets.insert(
            "accept-and-hold".into(),
            Widget::Builtin(BuiltinWidget::AcceptAndHold),
        );
        self.widgets.insert(
            "send-break".into(),
            Widget::Builtin(BuiltinWidget::SendBreak),
        );

        // Misc widgets
        self.widgets.insert(
            "clear-screen".into(),
            Widget::Builtin(BuiltinWidget::ClearScreen),
        );
        self.widgets.insert(
            "redisplay".into(),
            Widget::Builtin(BuiltinWidget::Redisplay),
        );
        self.widgets.insert(
            "transpose-chars".into(),
            Widget::Builtin(BuiltinWidget::TransposeChars),
        );
        self.widgets.insert(
            "transpose-words".into(),
            Widget::Builtin(BuiltinWidget::TransposeWords),
        );
        self.widgets.insert(
            "capitalize-word".into(),
            Widget::Builtin(BuiltinWidget::CapitalizeWord),
        );
        self.widgets.insert(
            "down-case-word".into(),
            Widget::Builtin(BuiltinWidget::DownCaseWord),
        );
        self.widgets.insert(
            "up-case-word".into(),
            Widget::Builtin(BuiltinWidget::UpCaseWord),
        );
        self.widgets.insert(
            "quoted-insert".into(),
            Widget::Builtin(BuiltinWidget::QuotedInsert),
        );
        self.widgets.insert(
            "vi-cmd-mode".into(),
            Widget::Builtin(BuiltinWidget::ViCmdMode),
        );
        self.widgets
            .insert("vi-insert".into(), Widget::Builtin(BuiltinWidget::ViInsert));
        self.widgets.insert(
            "set-mark-command".into(),
            Widget::Builtin(BuiltinWidget::SetMarkCommand),
        );
        self.widgets.insert(
            "exchange-point-and-mark".into(),
            Widget::Builtin(BuiltinWidget::ExchangePointAndMark),
        );
        self.widgets.insert(
            "kill-region".into(),
            Widget::Builtin(BuiltinWidget::KillRegion),
        );
        self.widgets.insert(
            "copy-region-as-kill".into(),
            Widget::Builtin(BuiltinWidget::CopyRegionAsKill),
        );
    }

    /// Setup default keymaps
    fn setup_default_keymaps(&mut self) {
        self.keymaps
            .insert(KeymapName::Emacs, Keymap::emacs_default());
        self.keymaps
            .insert(KeymapName::ViInsert, Keymap::viins_default());
        self.keymaps
            .insert(KeymapName::ViCommand, Keymap::vicmd_default());
        self.keymaps
            .insert(KeymapName::Main, Keymap::emacs_default());
    }

    /// Define a new user widget
    pub fn define_widget(&mut self, name: &str, function: &str) {
        self.user_widgets
            .insert(name.to_string(), function.to_string());
        self.widgets
            .insert(name.to_string(), Widget::User(function.to_string()));
    }

    /// Get a widget by name
    pub fn get_widget(&self, name: &str) -> Option<&Widget> {
        self.widgets.get(name)
    }

    /// List all widgets
    pub fn list_widgets(&self) -> Vec<&str> {
        self.widgets.keys().map(|s| s.as_str()).collect()
    }

    /// Execute a widget by name
    pub fn execute_widget(&mut self, name: &str, key: Option<char>) -> WidgetResult {
        let widget = match self.widgets.get(name) {
            Some(w) => w.clone(),
            None => return WidgetResult::Error(format!("widget not found: {}", name)),
        };

        self.state.last_widget = Some(name.to_string());

        match widget {
            Widget::Builtin(builtin) => widgets::execute_builtin(&mut self.state, builtin, key),
            Widget::User(func_name) => WidgetResult::CallFunction(func_name),
        }
    }

    /// Bind a key sequence to a widget
    pub fn bind_key(&mut self, keymap: KeymapName, keys: &str, widget: &str) {
        if let Some(km) = self.keymaps.get_mut(&keymap) {
            km.bind(keys, widget);
        }
    }

    /// Unbind a key sequence
    pub fn unbind_key(&mut self, keymap: KeymapName, keys: &str) {
        if let Some(km) = self.keymaps.get_mut(&keymap) {
            km.unbind(keys);
        }
    }

    /// Get the widget bound to a key sequence
    pub fn lookup_key(&self, keymap: KeymapName, keys: &str) -> Option<&str> {
        self.keymaps.get(&keymap).and_then(|km| km.lookup(keys))
    }

    /// Process a key input
    pub fn process_key(&mut self, key: char) -> WidgetResult {
        self.state.pending_keys.push(key);
        let key_str: String = self.state.pending_keys.iter().collect();

        let keymap = self.state.keymap;

        // Check for exact match
        if let Some(widget_name) = self.lookup_key(keymap, &key_str).map(|s| s.to_string()) {
            self.state.pending_keys.clear();
            return self.execute_widget(&widget_name, Some(key));
        }

        // Check if this could be a prefix of a longer binding
        if let Some(km) = self.keymaps.get(&keymap) {
            if km.has_prefix(&key_str) {
                return WidgetResult::Pending;
            }
        }

        // No match - execute self-insert for printable chars
        self.state.pending_keys.clear();
        if key.is_ascii_graphic() || key == ' ' {
            self.execute_widget("self-insert", Some(key))
        } else {
            WidgetResult::Ignored
        }
    }
}

/// Global ZLE instance
static ZLE_INSTANCE: std::sync::OnceLock<Mutex<Zle>> = std::sync::OnceLock::new();

/// Get the global ZLE instance
pub fn zle() -> std::sync::MutexGuard<'static, Zle> {
    ZLE_INSTANCE
        .get_or_init(|| Mutex::new(Zle::new()))
        .lock()
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zle_state_basic() {
        let mut state = ZleState::new();
        state.buffer = "hello world".to_string();
        state.cursor = 5;

        assert_eq!(state.lbuffer(), "hello");
        assert_eq!(state.rbuffer(), " world");
    }

    #[test]
    fn test_zle_undo() {
        let mut state = ZleState::new();
        state.buffer = "hello".to_string();
        state.cursor = 5;
        state.save_undo();

        state.buffer = "hello world".to_string();
        state.cursor = 11;

        assert!(state.undo());
        assert_eq!(state.buffer, "hello");
        assert_eq!(state.cursor, 5);
    }

    #[test]
    fn test_kill_ring() {
        let mut state = ZleState::new();
        state.kill_add("first");
        state.kill_add("second");

        assert_eq!(state.yank(), Some("second"));
        assert_eq!(state.yank_pop(), Some("first"));
    }
}
