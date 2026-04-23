//! ZLE refresh - screen redraw routines
//!
//! Direct port from zsh/Src/Zle/zle_refresh.c

use std::io::{self, Write};

use super::main::Zle;

/// Refresh parameters
#[derive(Debug, Clone, Default)]
pub struct RefreshState {
    /// Number of columns
    pub columns: usize,
    /// Number of lines  
    pub lines: usize,
    /// Current line on screen
    pub vln: usize,
    /// Current column on screen
    pub vcs: usize,
    /// Prompt width
    pub lpromptw: usize,
    /// Right prompt width
    pub rpromptw: usize,
    /// Scroll offset
    pub scrolloff: usize,
}

impl Zle {
    /// Full screen refresh
    pub fn full_refresh(&mut self) -> io::Result<()> {
        // Clear screen
        print!("\x1b[2J\x1b[H");
        
        // Draw prompt and line
        self.zrefresh();
        
        io::stdout().flush()
    }
    
    /// Partial refresh (optimize for minimal updates)
    pub fn partial_refresh(&mut self) -> io::Result<()> {
        // For now, just do full refresh
        self.zrefresh();
        io::stdout().flush()
    }
}

/// Get terminal size
pub fn get_terminal_size() -> (usize, usize) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(0, libc::TIOCGWINSZ, &mut ws) == 0 {
            (ws.ws_col as usize, ws.ws_row as usize)
        } else {
            (80, 24) // Default
        }
    }
}
