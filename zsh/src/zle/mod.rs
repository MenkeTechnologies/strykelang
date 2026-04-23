//! ZLE - Zsh Line Editor
//!
//! Direct port from zsh/Src/Zle/*.c
//!
//! This module implements the full Zsh line editor with:
//! - Vi and Emacs editing modes
//! - Programmable keymaps
//! - Widgets (commands)
//! - Completion integration
//! - History navigation
//! - Multi-line editing

// Core ZLE types (old API for exec.rs compatibility)
pub mod keymaps;
pub mod widgets;

// New comprehensive ZLE port from C
pub mod main;
pub mod keymap;
pub mod thingy;
pub mod widget;
pub mod refresh;
pub mod move_ops;
pub mod word;
pub mod misc;
pub mod hist;
pub mod vi;
pub mod utils;
pub mod params;
pub mod bindings;
pub mod textobjects;
pub mod tricky;
pub mod deltochar;
pub mod termquery;
pub mod zleparameter;
pub mod compcore_port;
pub mod complist_port;
pub mod compmatch_port;
pub mod compresult_port;
pub mod computil_port;

// Re-export old API for compatibility with exec.rs
pub use keymaps::{zle, Keymap as LegacyKeymap, KeymapName, ZleManager, ZleState};
pub use widgets::{BuiltinWidget, Widget as LegacyWidget, WidgetResult};

// Re-export new API
pub use main::Zle;
pub use keymap::{Keymap, KeymapManager};
pub use thingy::Thingy;
pub use widget::{Widget, WidgetFlags, WidgetFunc};
