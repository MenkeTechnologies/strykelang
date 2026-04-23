//! ZLE main routines - Direct port from zsh/Src/Zle/zle_main.c
//!
//! Core event loop, initialization, and main entry points for the line editor.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::{Duration, Instant};

use super::keymap::{Keymap, KeymapManager};
use super::thingy::Thingy;
use super::widget::{Widget, WidgetFlags};

/// ZLE character type - always char in Rust (Unicode native)
pub type ZleChar = char;

/// ZLE string type
pub type ZleString = Vec<ZleChar>;

/// ZLE integer type for character values
pub type ZleInt = i32;

/// EOF marker
pub const ZLEEOF: ZleInt = -1;

/// Flags for zleread()
#[derive(Debug, Clone, Copy, Default)]
pub struct ZleReadFlags {
    /// Don't add to history
    pub no_history: bool,
    /// Completion context
    pub completion: bool,
    /// We're in a vared context
    pub vared: bool,
}

/// Context for zleread()
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZleContext {
    #[default]
    Line,
    Cont,
    Select,
    Vared,
}

/// Modifier state for commands
#[derive(Debug, Clone, Default)]
pub struct Modifier {
    pub flags: ModifierFlags,
    /// Repeat count
    pub mult: i32,
    /// Repeat count being edited
    pub tmult: i32,
    /// Vi cut buffer
    pub vibuf: i32,
    /// Numeric base for digit arguments (usually 10)
    pub base: i32,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct ModifierFlags: u32 {
        /// A repeat count has been selected
        const MULT = 1 << 0;
        /// A repeat count is being entered
        const TMULT = 1 << 1;
        /// A vi cut buffer has been selected
        const VIBUF = 1 << 2;
        /// Appending to the vi cut buffer
        const VIAPP = 1 << 3;
        /// Last command was negate argument
        const NEG = 1 << 4;
        /// Throw away text for the vi cut buffer
        const NULL = 1 << 5;
        /// Force character-wise movement
        const CHAR = 1 << 6;
        /// Force line-wise movement
        const LINE = 1 << 7;
        /// OS primary selection for the vi cut buffer
        const PRI = 1 << 8;
        /// OS clipboard for the vi cut buffer
        const CLIP = 1 << 9;
    }
}

/// Undo change record
#[derive(Debug, Clone)]
pub struct Change {
    /// Flags (CH_NEXT, CH_PREV)
    pub flags: ChangeFlags,
    /// History line being changed
    pub hist: i32,
    /// Offset of the text changes
    pub off: usize,
    /// Characters to delete
    pub del: ZleString,
    /// Characters to insert
    pub ins: ZleString,
    /// Old cursor position
    pub old_cs: usize,
    /// New cursor position
    pub new_cs: usize,
    /// Unique change number
    pub changeno: u64,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct ChangeFlags: u32 {
        /// Next structure is also part of this change
        const NEXT = 1 << 0;
        /// Previous structure is also part of this change
        const PREV = 1 << 1;
    }
}

/// Watch file descriptor entry
#[derive(Debug, Clone)]
pub struct WatchFd {
    pub fd: RawFd,
    pub func: String,
}

/// Timeout type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutType {
    None,
    Key,
    Func,
    Max,
}

/// Timeout state
#[derive(Debug, Clone)]
pub struct Timeout {
    pub tp: TimeoutType,
    /// Value in 100ths of a second
    pub exp100ths: u64,
}

/// Maximum timeout value (about 24 days in 100ths of a second)
pub const ZMAXTIMEOUT: u64 = 1 << 21;

/// The main ZLE state
pub struct Zle {
    /// The input line assembled so far
    pub zleline: ZleString,
    /// Cursor position
    pub zlecs: usize,
    /// Line length
    pub zlell: usize,
    /// Mark position
    pub mark: usize,
    /// Insert mode (true) or overwrite mode (false)
    pub insmode: bool,
    /// Done editing flag
    pub done: bool,
    /// Last character pressed
    pub lastchar: ZleInt,
    /// Last character as wide char (always used in Rust)
    pub lastchar_wide: ZleInt,
    /// Whether lastchar_wide is valid
    pub lastchar_wide_valid: bool,
    /// Binding for the previous key
    pub lbindk: Option<Thingy>,
    /// Binding for this key
    pub bindk: Option<Thingy>,
    /// Flags associated with last command
    pub lastcmd: WidgetFlags,
    /// Current modifier status
    pub zmod: Modifier,
    /// Prefix command flag
    pub prefixflag: bool,
    /// Recursive edit depth
    pub zle_recursive: i32,
    /// Read flags
    pub zlereadflags: ZleReadFlags,
    /// Context
    pub zlecontext: ZleContext,
    /// Status line
    pub statusline: Option<String>,
    /// History position for buffer stack
    pub stackhist: i32,
    /// Cursor position for buffer stack
    pub stackcs: usize,
    /// Vi start change position in undo stack
    pub vistartchange: u64,
    /// Undo stack
    pub undo_stack: Vec<Change>,
    /// Current change number
    pub changeno: u64,
    /// Unget buffer for bytes
    unget_buf: VecDeque<u8>,
    /// EOF character
    eofchar: u8,
    /// EOF sent flag
    eofsent: bool,
    /// Key timeout in 100ths of a second
    pub keytimeout: u64,
    /// Terminal baud rate
    baud: u32,
    /// Watch file descriptors
    pub watch_fds: Vec<WatchFd>,
    /// Keymap manager
    pub keymaps: KeymapManager,
    /// Completion widget
    pub compwidget: Option<Widget>,
    /// In completion function flag
    pub incompctlfunc: bool,
    /// Completion module loaded flag
    pub hascompmod: bool,
    /// Terminal file descriptor
    ttyfd: RawFd,
    /// Left prompt
    lprompt: String,
    /// Right prompt
    rprompt: String,
    /// Pre-ZLE status
    pre_zle_status: i32,
    /// Needs refresh
    pub resetneeded: bool,
    /// Vi cut buffers (0-35: 0-9, a-z)
    pub vibuf: [ZleString; 36],
    /// Kill ring
    pub killring: VecDeque<ZleString>,
    /// Kill ring max size
    pub killringmax: usize,
}

impl Default for Zle {
    fn default() -> Self {
        Self::new()
    }
}

impl Zle {
    pub fn new() -> Self {
        Zle {
            zleline: Vec::new(),
            zlecs: 0,
            zlell: 0,
            mark: 0,
            insmode: true,
            done: false,
            lastchar: 0,
            lastchar_wide: 0,
            lastchar_wide_valid: false,
            lbindk: None,
            bindk: None,
            lastcmd: WidgetFlags::empty(),
            zmod: Modifier::default(),
            prefixflag: false,
            zle_recursive: 0,
            zlereadflags: ZleReadFlags::default(),
            zlecontext: ZleContext::default(),
            statusline: None,
            stackhist: 0,
            stackcs: 0,
            vistartchange: 0,
            undo_stack: Vec::new(),
            changeno: 0,
            unget_buf: VecDeque::new(),
            eofchar: 4, // Ctrl-D
            eofsent: false,
            keytimeout: 40, // 0.4 seconds default
            baud: 38400,
            watch_fds: Vec::new(),
            keymaps: KeymapManager::new(),
            compwidget: None,
            incompctlfunc: false,
            hascompmod: false,
            ttyfd: 0, // stdin
            lprompt: String::new(),
            rprompt: String::new(),
            pre_zle_status: 0,
            resetneeded: false,
            vibuf: std::array::from_fn(|_| Vec::new()),
            killring: VecDeque::new(),
            killringmax: 8,
        }
    }

    /// Set up terminal for ZLE
    pub fn zsetterm(&mut self) -> io::Result<()> {
        use std::os::unix::io::FromRawFd;
        
        // Get current terminal settings
        let mut termios = termios::Termios::from_fd(self.ttyfd)?;
        
        // Save original settings (would need to store for restore)
        
        // Set raw mode
        termios.c_lflag &= !(termios::ICANON | termios::ECHO);
        termios.c_cc[termios::VMIN] = 1;
        termios.c_cc[termios::VTIME] = 0;
        
        // Apply settings
        termios::tcsetattr(self.ttyfd, termios::TCSANOW, &termios)?;
        
        Ok(())
    }

    /// Unget a byte back to the input buffer
    pub fn ungetbyte(&mut self, ch: u8) {
        self.unget_buf.push_front(ch);
    }

    /// Unget multiple bytes
    pub fn ungetbytes(&mut self, s: &[u8]) {
        for &b in s.iter().rev() {
            self.unget_buf.push_front(b);
        }
    }

    /// Calculate timeout for input
    fn calc_timeout(&self, do_keytmout: bool) -> Timeout {
        if do_keytmout && self.keytimeout > 0 {
            let exp = if self.keytimeout > ZMAXTIMEOUT * 100 {
                ZMAXTIMEOUT * 100
            } else {
                self.keytimeout
            };
            Timeout {
                tp: TimeoutType::Key,
                exp100ths: exp,
            }
        } else {
            Timeout {
                tp: TimeoutType::None,
                exp100ths: 0,
            }
        }
    }

    /// Read a raw byte from input with optional timeout
    pub fn raw_getbyte(&mut self, do_keytmout: bool) -> Option<u8> {
        // Check unget buffer first
        if let Some(b) = self.unget_buf.pop_front() {
            return Some(b);
        }

        let timeout = self.calc_timeout(do_keytmout);
        
        let timeout_duration = if timeout.tp != TimeoutType::None {
            Some(Duration::from_millis(timeout.exp100ths * 10))
        } else {
            None
        };

        // Use poll/select to wait for input with timeout
        let mut buf = [0u8; 1];
        
        if let Some(dur) = timeout_duration {
            // Set up poll
            let start = Instant::now();
            loop {
                if start.elapsed() >= dur {
                    return None; // Timeout
                }
                
                // Try non-blocking read
                match self.try_read_byte(&mut buf) {
                    Ok(true) => return Some(buf[0]),
                    Ok(false) => {
                        // No data, sleep a bit and retry
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => return None,
                }
            }
        } else {
            // Blocking read
            match io::stdin().read(&mut buf) {
                Ok(1) => Some(buf[0]),
                _ => None,
            }
        }
    }

    /// Try to read a byte non-blocking
    fn try_read_byte(&self, buf: &mut [u8]) -> io::Result<bool> {
        use std::os::unix::io::AsRawFd;
        
        let mut fds = [libc::pollfd {
            fd: io::stdin().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        
        let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, 0) };
        
        if ret > 0 && (fds[0].revents & libc::POLLIN) != 0 {
            match io::stdin().read(buf) {
                Ok(1) => Ok(true),
                Ok(_) => Ok(false),
                Err(e) => Err(e),
            }
        } else {
            Ok(false)
        }
    }

    /// Get a byte from input, handling timeout
    pub fn getbyte(&mut self, do_keytmout: bool) -> Option<u8> {
        let b = self.raw_getbyte(do_keytmout)?;
        
        // Handle newline/carriage return translation
        // (The C code swaps \n and \r for typeahead handling)
        let b = if b == b'\n' { b'\r' } else if b == b'\r' { b'\n' } else { b };
        
        self.lastchar = b as ZleInt;
        Some(b)
    }

    /// Get a full (possibly wide) character - always returns char in Rust
    pub fn getfullchar(&mut self, do_keytmout: bool) -> Option<char> {
        let b = self.getbyte(do_keytmout)?;
        
        // UTF-8 decoding
        if b < 0x80 {
            let c = b as char;
            self.lastchar_wide = c as ZleInt;
            self.lastchar_wide_valid = true;
            return Some(c);
        }
        
        // Multi-byte UTF-8
        let mut bytes = vec![b];
        let expected_len = if b < 0xE0 {
            2
        } else if b < 0xF0 {
            3
        } else {
            4
        };
        
        while bytes.len() < expected_len {
            if let Some(next) = self.getbyte(true) {
                if (next & 0xC0) != 0x80 {
                    // Invalid continuation byte, unget and return error
                    self.ungetbyte(next);
                    break;
                }
                bytes.push(next);
            } else {
                break;
            }
        }
        
        match std::str::from_utf8(&bytes) {
            Ok(s) => {
                if let Some(c) = s.chars().next() {
                    self.lastchar_wide = c as ZleInt;
                    self.lastchar_wide_valid = true;
                    return Some(c);
                }
            }
            Err(_) => {}
        }
        
        self.lastchar_wide_valid = false;
        None
    }

    /// Redraw hook
    pub fn redrawhook(&mut self) {
        // Call redraw hook functions
        // TODO: implement hook system
    }

    /// Core ZLE loop
    pub fn zlecore(&mut self) {
        self.done = false;
        
        while !self.done {
            // Reset prefix flag
            if !self.prefixflag {
                self.zmod = Modifier::default();
            }
            self.prefixflag = false;
            
            // Get next key
            let c = match self.getfullchar(false) {
                Some(c) => c,
                None => {
                    self.done = true;
                    continue;
                }
            };
            
            // Look up binding
            let key = c;
            
            if let Some(thingy) = self.keymaps.lookup_key(key) {
                self.lbindk = self.bindk.take();
                self.bindk = Some(thingy.clone());
                
                // Execute the widget
                if let Some(widget) = &thingy.widget {
                    self.execute_widget(widget);
                }
            } else {
                // Self-insert
                self.self_insert(key);
            }
            
            // Refresh display if needed
            if self.resetneeded {
                self.zrefresh();
                self.resetneeded = false;
            }
        }
    }

    /// Execute a widget
    fn execute_widget(&mut self, widget: &Widget) {
        self.lastcmd = widget.flags;
        
        match &widget.func {
            super::widget::WidgetFunc::Internal(f) => {
                f(self);
            }
            super::widget::WidgetFunc::User(name) => {
                // Call user-defined widget (shell function)
                // TODO: implement user widget execution
                let _ = name;
            }
        }
    }

    /// Self-insert character
    pub fn self_insert(&mut self, c: char) {
        if self.insmode {
            // Insert mode
            self.zleline.insert(self.zlecs, c);
            self.zlecs += 1;
            self.zlell += 1;
        } else {
            // Overwrite mode
            if self.zlecs < self.zlell {
                self.zleline[self.zlecs] = c;
            } else {
                self.zleline.push(c);
                self.zlell += 1;
            }
            self.zlecs += 1;
        }
        self.resetneeded = true;
    }

    /// Refresh the display
    pub fn zrefresh(&mut self) {
        // TODO: implement full refresh logic from zle_refresh.c
        // For now, simple line redraw
        print!("\r\x1b[K{}{}", self.lprompt, self.zleline.iter().collect::<String>());
        
        // Position cursor
        let cursor_pos = self.lprompt.len() + self.zlecs;
        print!("\r\x1b[{}C", cursor_pos);
        
        io::stdout().flush().ok();
    }

    /// Main entry point for line reading
    pub fn zleread(
        &mut self,
        lprompt: &str,
        rprompt: &str,
        flags: ZleReadFlags,
        context: ZleContext,
    ) -> io::Result<String> {
        self.lprompt = lprompt.to_string();
        self.rprompt = rprompt.to_string();
        self.zlereadflags = flags;
        self.zlecontext = context;
        
        // Initialize line
        self.zleline.clear();
        self.zlecs = 0;
        self.zlell = 0;
        self.mark = 0;
        self.done = false;
        
        // Set up terminal
        self.zsetterm()?;
        
        // Display prompt
        print!("{}", lprompt);
        io::stdout().flush()?;
        
        // Enter core loop
        self.zlecore();
        
        // Return the line
        Ok(self.zleline.iter().collect())
    }

    /// Initialize ZLE modifiers
    pub fn initmodifier(&mut self) {
        self.zmod = Modifier {
            flags: ModifierFlags::empty(),
            mult: 1,
            tmult: 0,
            vibuf: -1,
            base: 10,
        };
    }

    /// Handle prefix commands
    pub fn handleprefixes(&mut self) {
        if self.zmod.flags.contains(ModifierFlags::TMULT) {
            self.zmod.flags.remove(ModifierFlags::TMULT);
            self.zmod.flags.insert(ModifierFlags::MULT);
            self.zmod.mult = self.zmod.tmult;
        }
    }

    /// Trash the ZLE display
    pub fn trashzle(&mut self) {
        print!("\r\x1b[K");
        io::stdout().flush().ok();
    }

    /// Reset prompt
    pub fn resetprompt(&mut self) {
        self.resetneeded = true;
    }

    /// Re-expand prompt
    pub fn reexpandprompt(&mut self) {
        // TODO: implement prompt expansion
        self.resetneeded = true;
    }

    /// Recursive edit
    pub fn recursive_edit(&mut self) -> i32 {
        self.zle_recursive += 1;
        
        let old_done = self.done;
        self.done = false;
        
        self.zlecore();
        
        self.done = old_done;
        self.zle_recursive -= 1;
        
        0
    }

    /// Accept line
    pub fn accept_line(&mut self) {
        self.done = true;
    }

    /// Send break
    pub fn send_break(&mut self) {
        self.zleline.clear();
        self.zlecs = 0;
        self.zlell = 0;
        self.done = true;
    }
}

/// Module for termios operations
mod termios {
    pub use libc::{ECHO, ICANON, TCSANOW, VMIN, VTIME};
    use std::io;
    use std::os::unix::io::RawFd;

    #[derive(Clone)]
    pub struct Termios {
        inner: libc::termios,
    }

    impl Termios {
        pub fn from_fd(fd: RawFd) -> io::Result<Self> {
            let mut termios = std::mem::MaybeUninit::uninit();
            let ret = unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) };
            if ret != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Termios {
                inner: unsafe { termios.assume_init() },
            })
        }
    }

    impl std::ops::Deref for Termios {
        type Target = libc::termios;
        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    impl std::ops::DerefMut for Termios {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.inner
        }
    }

    pub fn tcsetattr(fd: RawFd, action: i32, termios: &Termios) -> io::Result<()> {
        let ret = unsafe { libc::tcsetattr(fd, action, &termios.inner) };
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
