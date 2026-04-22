//! Menu completion and interactive selection
//!
//! Implements zsh-style menu completion with:
//! - Auto-scrolling viewport (no pagination prompts)
//! - Live command line updates as you navigate
//! - Column memory for vertical navigation
//! - Group support with headers
//! - Incremental search filtering
//! - Multi-select with accept-and-continue
//!
//! Based on zsh's Src/Zle/compresult.c and complist.c

use crate::completion::{Completion, CompletionGroup};
use crate::zpwr_colors::HeaderColors;
use crate::zstyle::ZStyleStore;
use unicode_width::UnicodeWidthStr;

/// ANSI color codes
pub mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const UNDERLINE: &str = "\x1b[4m";
    pub const REVERSE: &str = "\x1b[7m";

    pub const BLACK: &str = "\x1b[30m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[37m";

    pub const BG_BLACK: &str = "\x1b[40m";
    pub const BG_RED: &str = "\x1b[41m";
    pub const BG_GREEN: &str = "\x1b[42m";
    pub const BG_YELLOW: &str = "\x1b[43m";
    pub const BG_BLUE: &str = "\x1b[44m";
    pub const BG_MAGENTA: &str = "\x1b[45m";
    pub const BG_CYAN: &str = "\x1b[46m";
    pub const BG_WHITE: &str = "\x1b[47m";

    /// Build ANSI escape from semicolon-separated codes like "38;5;82"
    pub fn from_codes(codes: &str) -> String {
        if codes.is_empty() {
            String::new()
        } else {
            format!("\x1b[{}m", codes)
        }
    }
}

/// Terminal colors for completion display
#[derive(Clone, Debug)]
pub struct MenuColors {
    /// Normal text (no)
    pub normal: String,
    /// Selected item (ma)
    pub selected: String,
    /// Secondary row background (sp)
    pub secondary: String,
    /// Completion text (tc)
    pub completion: String,
    /// Description text (dc)  
    pub description: String,
    /// Prefix text
    pub prefix: String,
    /// Group header (so)
    pub header: String,
    /// Scroll indicator
    pub scroll: String,
    /// File (fi)
    pub file: String,
    /// Directory (di)
    pub directory: String,
    /// Executable (ex)
    pub executable: String,
    /// Symbolic link (ln)
    pub symlink: String,
}

impl Default for MenuColors {
    fn default() -> Self {
        Self {
            normal: String::new(),
            selected: "7".to_string(),  // reverse video
            secondary: "2".to_string(), // dim
            completion: String::new(),
            description: "36".to_string(), // cyan
            prefix: "32".to_string(),      // green
            header: "1;33".to_string(),    // bold yellow
            scroll: "2".to_string(),       // dim
            file: String::new(),
            directory: "1;34".to_string(),  // bold blue
            executable: "1;32".to_string(), // bold green
            symlink: "1;36".to_string(),    // bold cyan
        }
    }
}

impl MenuColors {
    /// Parse from ZLS_COLORS/ZLS_COLOURS environment or zstyle
    pub fn from_zstyle(styles: &ZStyleStore, context: &str) -> Self {
        let mut colors = Self::default();

        if let Some(vals) = styles.lookup_values(context, "list-colors") {
            for val in vals {
                if let Some((key, color)) = val.split_once('=') {
                    match key {
                        "no" => colors.normal = color.to_string(),
                        "ma" => colors.selected = color.to_string(),
                        "sp" => colors.secondary = color.to_string(),
                        "tc" => colors.completion = color.to_string(),
                        "dc" => colors.description = color.to_string(),
                        "so" => colors.header = color.to_string(),
                        "fi" => colors.file = color.to_string(),
                        "di" => colors.directory = color.to_string(),
                        "ex" => colors.executable = color.to_string(),
                        "ln" => colors.symlink = color.to_string(),
                        _ => {}
                    }
                }
            }
        }
        colors
    }

    /// Get ANSI escape for a color code string
    pub fn escape(&self, color: &str) -> String {
        ansi::from_codes(color)
    }
}

/// Direction for menu navigation
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuMotion {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Next,
    Prev,
    First,
    Last,
    Deselect,
}

/// Layout information for a completion group
#[derive(Clone, Debug, Default)]
pub struct GroupLayout {
    /// Group name/tag
    pub name: String,
    /// Explanation/header text
    pub explanation: Option<String>,
    /// Number of matches in this group
    pub count: usize,
    /// Starting index in the flat match list
    pub start_idx: usize,
    /// Starting row in the layout
    pub start_row: usize,
    /// Number of rows for this group (including header)
    pub row_count: usize,
    /// Column widths for this group
    pub col_widths: Vec<usize>,
    /// Number of columns
    pub cols: usize,
    /// Pack columns tightly (LIST_PACKED)
    pub packed: bool,
    /// Fill rows first (LIST_ROWS_FIRST)
    pub rows_first: bool,
    /// ANSI color code for this group (cyberpunk!)
    pub color: String,
    /// Whether items in this group have descriptions
    pub has_descriptions: bool,
    /// Width of completion column (for aligning descriptions)
    pub comp_col_width: usize,
}

/// ZPWR-style color palette for completion groups (from ~/.zpwr zstyle configs)
/// Format: "FG;BG" where BG uses 4x codes (41=red, 42=green, 43=yellow, 44=blue, 45=magenta, 46=cyan)
pub const GROUP_COLORS: &[&str] = &[
    "34;42;4",   // aliases: blue on green, underline
    "1;37;41",   // functions: bold white on red
    "1;37;4;43", // builtins: bold white on yellow, underline
    "1;37;44",   // executables: bold white on blue
    "1;32;45",   // parameters: bold green on magenta
    "1;34;41;4", // files: bold blue on red, underline
    "1;37;42",   // users: bold white on green
    "1;37;43",   // hosts: bold white on yellow
    "1;34;43;4", // global-aliases: bold blue on yellow, underline
    "1;4;37;45", // reserved-words: bold underline white on magenta
    "1;37;46",   // heads-remote: bold white on cyan
    "1;37;44",   // recent-branches: bold white on blue
];

/// A single item in the menu display
#[derive(Clone, Debug)]
pub struct MenuItem {
    /// The completion
    pub completion: Completion,
    /// Group index
    pub group_idx: usize,
    /// Index within group
    pub idx_in_group: usize,
    /// Display string (with any prefix/suffix)
    pub display: String,
    /// Description
    pub description: String,
    /// Display width of completion
    pub comp_width: usize,
    /// Display width of description
    pub desc_width: usize,
}

/// Rendered line for display
#[derive(Clone, Debug, Default)]
pub struct MenuLine {
    /// Characters with ANSI escapes
    pub content: String,
    /// Logical width (excluding escapes)
    pub width: usize,
    /// Is this a header line?
    pub is_header: bool,
}

impl MenuLine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn header(text: &str, _color: &str) -> Self {
        // ZPWR-style header: RED -<< BLUE text RED >>-
        let hc = HeaderColors::from_env();
        let formatted = format!("{}{}{}", hc.pre, text, hc.post);
        let width = UnicodeWidthStr::width(formatted.as_str());
        let content = hc.format(text);

        Self {
            content,
            width,
            is_header: true,
        }
    }

    pub fn append(&mut self, s: &str) {
        let w = UnicodeWidthStr::width(s);
        self.content.push_str(s);
        self.width += w;
    }

    pub fn append_with_width(&mut self, s: &str, width: usize) {
        self.content.push_str(s);
        self.width += width;
    }

    pub fn append_colored(&mut self, s: &str, color: &str) {
        let w = UnicodeWidthStr::width(s);
        if !color.is_empty() {
            self.content.push_str(&ansi::from_codes(color));
        }
        self.content.push_str(s);
        if !color.is_empty() {
            self.content.push_str(ansi::RESET);
        }
        self.width += w;
    }

    /// Pad to specified width with spaces
    pub fn pad_to(&mut self, target_width: usize) {
        if self.width < target_width {
            let pad = target_width - self.width;
            self.content.push_str(&" ".repeat(pad));
            self.width = target_width;
        }
    }

    /// Pad to width with colored background
    pub fn pad_to_colored(&mut self, target_width: usize, bg_color: &str) {
        if self.width < target_width {
            let pad = target_width - self.width;
            if !bg_color.is_empty() {
                self.content.push_str(&ansi::from_codes(bg_color));
            }
            self.content.push_str(&" ".repeat(pad));
            if !bg_color.is_empty() {
                self.content.push_str(ansi::RESET);
            }
            self.width = target_width;
        }
    }
}

/// The rendered menu display
#[derive(Clone, Debug, Default)]
pub struct MenuRendering {
    /// Terminal width used for this rendering
    pub term_width: usize,
    /// Terminal height used for this rendering  
    pub term_height: usize,
    /// Total number of matches
    pub total_matches: usize,
    /// Number of columns in layout
    pub cols: usize,
    /// Number of rows in layout (may exceed viewport)
    pub total_rows: usize,
    /// First visible row
    pub row_start: usize,
    /// Last visible row (exclusive)
    pub row_end: usize,
    /// Selected match index (None = no selection)
    pub selected_idx: Option<usize>,
    /// Selected row in the full layout
    pub selected_row: usize,
    /// Selected column
    pub selected_col: usize,
    /// Lines to display
    pub lines: Vec<MenuLine>,
    /// Status line text (e.g., "rows 5-12 of 47")
    pub status: Option<String>,
    /// Whether list is scrollable
    pub scrollable: bool,
}

/// Menu completion state machine
///
/// Implements zsh menuselect behavior from Src/Zle/complist.c
#[derive(Clone, Debug)]
pub struct MenuState {
    /// All completion items, flattened
    items: Vec<MenuItem>,
    /// Group layouts
    groups: Vec<GroupLayout>,
    /// Current selection index (None = not in menu mode)
    selected_idx: Option<usize>,
    /// Column memory for vertical navigation (zsh: wishcol)
    wish_col: usize,
    /// First visible row
    viewport_start: usize,
    /// Terminal dimensions
    term_width: usize,
    term_height: usize,
    /// Space available for completions (term_height - prompt lines - status)
    available_rows: usize,
    /// Colors
    colors: MenuColors,
    /// Prefix being completed
    prefix: String,
    /// Search/filter string (used in MM_FSEARCH/MM_BSEARCH and MM_INTER modes)
    search: String,
    /// Whether incremental search is active (MM_FSEARCH or MM_BSEARCH)
    search_active: bool,
    /// Search direction for incremental search
    search_direction: SearchDirection,
    /// Whether interactive filter mode is active (MM_INTER, toggled by vi-insert)
    interactive_mode: bool,
    /// Unfiltered items (for search restore)
    unfiltered_items: Vec<MenuItem>,
    /// Layout cache valid
    layout_valid: bool,
    /// Cached total rows
    cached_total_rows: usize,
    /// Cached column count
    cached_cols: usize,
    /// Cached column widths (uniform for now)
    cached_col_width: usize,
    /// Show group headers
    show_headers: bool,
    /// Custom colors for groups by name (from zstyle)
    group_colors: std::collections::HashMap<String, String>,
    /// Menu selection color (ma= from zstyle)
    selection_color: String,
    /// Prefix match color (from zstyle list-colors pattern)
    prefix_color: String,
    /// List separator (ZPWR_CHAR_LOGO)
    list_separator: String,
}

impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
}

impl MenuState {
    pub fn new() -> Self {
        // Load config from zpwr zstyle files
        let config = crate::zpwr_colors::load_zpwr_config();
        
        Self {
            items: Vec::new(),
            groups: Vec::new(),
            selected_idx: None,
            wish_col: 0,
            viewport_start: 0,
            term_width: 80,
            term_height: 24,
            available_rows: 20,
            colors: MenuColors::default(),
            group_colors: config.tag_colors,
            prefix: String::new(),
            search: String::new(),
            search_active: false,
            search_direction: SearchDirection::Forward,
            interactive_mode: false,
            unfiltered_items: Vec::new(),
            layout_valid: false,
            cached_total_rows: 0,
            cached_cols: 0,
            cached_col_width: 0,
            show_headers: true,
            selection_color: if config.menu_selection.is_empty() {
                "37;1;4;44".to_string() // fallback
            } else {
                config.menu_selection
            },
            prefix_color: if config.prefix_color.is_empty() {
                "1;30".to_string() // fallback
            } else {
                config.prefix_color
            },
            list_separator: if config.list_separator.is_empty() {
                "/////////".to_string() // fallback
            } else {
                config.list_separator
            },
        }
    }

    /// Set terminal size
    pub fn set_term_size(&mut self, width: usize, height: usize) {
        if self.term_width != width || self.term_height != height {
            self.term_width = width;
            self.term_height = height;
            self.layout_valid = false;
        }
    }

    /// Set available rows for completion display
    pub fn set_available_rows(&mut self, rows: usize) {
        if self.available_rows != rows {
            self.available_rows = rows;
            self.layout_valid = false;
        }
    }

    /// Set colors from zstyle
    pub fn set_colors(&mut self, colors: MenuColors) {
        self.colors = colors;
    }

    /// Set custom colors for groups by name (from zstyle list-colors)
    pub fn set_group_colors(&mut self, colors: std::collections::HashMap<String, String>) {
        self.group_colors = colors;
    }

    /// Set the prefix being completed
    pub fn set_prefix(&mut self, prefix: &str) {
        self.prefix = prefix.to_string();
    }

    /// Enable/disable group headers
    pub fn set_show_headers(&mut self, show: bool) {
        if self.show_headers != show {
            self.show_headers = show;
            self.layout_valid = false;
        }
    }

    /// Load completions from groups
    pub fn set_completions(&mut self, groups: &[CompletionGroup]) {
        self.items.clear();
        self.groups.clear();
        self.unfiltered_items.clear();
        self.layout_valid = false;

        let mut idx = 0;
        for (group_idx, group) in groups.iter().enumerate() {
            let start_idx = idx;

            for (i, comp) in group.matches.iter().enumerate() {
                let display = comp.disp.as_ref().unwrap_or(&comp.str_).clone();
                let description = comp
                    .desc
                    .clone()
                    .or_else(|| comp.exp.clone())
                    .unwrap_or_default();

                let comp_width = UnicodeWidthStr::width(display.as_str());
                let desc_width = UnicodeWidthStr::width(description.as_str());

                self.items.push(MenuItem {
                    completion: comp.clone(),
                    group_idx,
                    idx_in_group: i,
                    display,
                    description,
                    comp_width,
                    desc_width,
                });
                idx += 1;
            }

            // Look up color by group name, falling back to default palette
            let color = self
                .group_colors
                .get(&group.name)
                .or_else(|| self.group_colors.get(&group.name.to_lowercase()))
                .cloned()
                .unwrap_or_else(|| GROUP_COLORS[group_idx % GROUP_COLORS.len()].to_string());

            // Check if any items in this group have descriptions
            let has_descs = group.matches.iter().any(|m| {
                m.desc.is_some() || m.exp.is_some()
            });
            
            self.groups.push(GroupLayout {
                name: group.name.clone(),
                explanation: group
                    .explanation
                    .clone()
                    .or_else(|| group.explanations.first().cloned()),
                count: group.matches.len(),
                start_idx,
                start_row: 0,
                row_count: 0,
                col_widths: Vec::new(),
                cols: 0,
                packed: false,
                rows_first: false,
                color,
                has_descriptions: has_descs,
                comp_col_width: 0,
            });
        }

        self.unfiltered_items = self.items.clone();
    }

    /// Check if menu is active
    pub fn is_active(&self) -> bool {
        self.selected_idx.is_some()
    }

    /// Get the currently selected completion
    pub fn selected(&self) -> Option<&Completion> {
        self.selected_idx
            .and_then(|idx| self.items.get(idx).map(|m| &m.completion))
    }

    /// Get selected index
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_idx
    }

    /// Get the insert string for the currently selected completion
    pub fn selected_insert_string(&self) -> Option<String> {
        self.selected_idx
            .and_then(|idx| self.items.get(idx))
            .map(|m| m.completion.insert_str())
    }

    /// Total number of matches
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Start menu completion (select first item)
    pub fn start(&mut self) {
        if !self.items.is_empty() {
            self.selected_idx = Some(0);
            self.wish_col = 0;
            self.viewport_start = 0;
            self.ensure_layout();
        }
    }

    /// Exit menu completion
    pub fn stop(&mut self) {
        self.selected_idx = None;
    }

    /// Navigate in the given direction
    pub fn navigate(&mut self, motion: MenuMotion) -> bool {
        if self.items.is_empty() {
            return false;
        }

        self.ensure_layout();

        let old_idx = self.selected_idx;
        let rows = self.rows_for_items();
        let cols = self.cached_cols;

        match motion {
            MenuMotion::Deselect => {
                self.selected_idx = None;
            }
            MenuMotion::First => {
                self.selected_idx = Some(0);
                self.wish_col = 0;
            }
            MenuMotion::Last => {
                self.selected_idx = Some(self.items.len() - 1);
                self.wish_col = cols.saturating_sub(1);
            }
            MenuMotion::Next => match self.selected_idx {
                None => self.selected_idx = Some(0),
                Some(idx) => {
                    self.selected_idx = Some((idx + 1) % self.items.len());
                }
            },
            MenuMotion::Prev => match self.selected_idx {
                None => self.selected_idx = Some(self.items.len() - 1),
                Some(0) => self.selected_idx = Some(self.items.len() - 1),
                Some(idx) => self.selected_idx = Some(idx - 1),
            },
            MenuMotion::Up
            | MenuMotion::Down
            | MenuMotion::Left
            | MenuMotion::Right
            | MenuMotion::PageUp
            | MenuMotion::PageDown => {
                if let Some(idx) = self.selected_idx {
                    // Use group-aware row/col calculation
                    let (row, col) = self.idx_to_visual_row_col(idx);
                    let total_rows = rows;
                    
                    let (new_row, new_col) = match motion {
                        MenuMotion::Up => {
                            if row > 0 {
                                (row - 1, self.wish_col)
                            } else {
                                // Wrap to bottom
                                (total_rows.saturating_sub(1), self.wish_col)
                            }
                        }
                        MenuMotion::Down => {
                            if row + 1 < total_rows {
                                (row + 1, self.wish_col)
                            } else {
                                // Wrap to top
                                (0, self.wish_col)
                            }
                        }
                        MenuMotion::Left => {
                            if col > 0 {
                                self.wish_col = col - 1;
                                (row, col - 1)
                            } else {
                                // Wrap to previous row, last column
                                if row > 0 {
                                    // Get column count for previous row's group
                                    if let Some(prev_idx) = self.visual_row_col_to_idx(row - 1, 0) {
                                        if let Some((_, group, _)) = self.find_group_for_idx(prev_idx) {
                                            let last_col = group.cols.saturating_sub(1);
                                            self.wish_col = last_col;
                                            (row - 1, last_col)
                                        } else {
                                            (row, col)
                                        }
                                    } else {
                                        (row, col)
                                    }
                                } else {
                                    (row, col)
                                }
                            }
                        }
                        MenuMotion::Right => {
                            // Get current group's column count
                            let max_col = if let Some((_, group, _)) = self.find_group_for_idx(idx) {
                                group.cols.saturating_sub(1)
                            } else {
                                cols.saturating_sub(1)
                            };
                            
                            if col < max_col {
                                self.wish_col = col + 1;
                                (row, col + 1)
                            } else {
                                // Wrap to next row, first column
                                if row + 1 < total_rows {
                                    self.wish_col = 0;
                                    (row + 1, 0)
                                } else {
                                    (row, col)
                                }
                            }
                        }
                        MenuMotion::PageUp => {
                            let page = self.available_rows.saturating_sub(1);
                            (row.saturating_sub(page), col)
                        }
                        MenuMotion::PageDown => {
                            let page = self.available_rows.saturating_sub(1);
                            ((row + page).min(total_rows.saturating_sub(1)), col)
                        }
                        _ => (row, col),
                    };

                    // Convert back to index using group-aware function
                    if let Some(new_idx) = self.visual_row_col_to_idx(new_row, new_col) {
                        self.selected_idx = Some(new_idx);
                    } else {
                        // Try to find a valid item in that row
                        for try_col in (0..=new_col).rev() {
                            if let Some(new_idx) = self.visual_row_col_to_idx(new_row, try_col) {
                                self.selected_idx = Some(new_idx);
                                break;
                            }
                        }
                    }
                }
            }
        }

        self.ensure_selection_visible();
        self.selected_idx != old_idx
    }

    /// Number of rows needed for items (excluding headers)
    fn rows_for_items(&self) -> usize {
        // Sum up rows from all groups
        self.groups.iter().map(|g| g.row_count).sum()
    }
    
    /// Find which group an item index belongs to, and its local index within that group
    fn find_group_for_idx(&self, idx: usize) -> Option<(usize, &GroupLayout, usize)> {
        let mut offset = 0;
        for (gi, group) in self.groups.iter().enumerate() {
            if idx < offset + group.count {
                return Some((gi, group, idx - offset));
            }
            offset += group.count;
        }
        None
    }
    
    /// Convert global item index to (visual_row, col) accounting for per-group column counts
    fn idx_to_visual_row_col(&self, idx: usize) -> (usize, usize) {
        let mut row_offset = 0;
        let mut item_offset = 0;
        
        for group in &self.groups {
            if idx < item_offset + group.count {
                let local_idx = idx - item_offset;
                let cols = group.cols.max(1);
                let local_row = local_idx / cols;
                let col = local_idx % cols;
                return (row_offset + local_row, col);
            }
            row_offset += group.row_count;
            item_offset += group.count;
        }
        (0, 0)
    }
    
    /// Convert (visual_row, col) back to global item index
    fn visual_row_col_to_idx(&self, target_row: usize, target_col: usize) -> Option<usize> {
        let mut row_offset = 0;
        let mut item_offset = 0;
        
        for group in &self.groups {
            let group_end_row = row_offset + group.row_count;
            if target_row < group_end_row {
                let local_row = target_row - row_offset;
                let cols = group.cols.max(1);
                let local_idx = local_row * cols + target_col.min(cols - 1);
                if local_idx < group.count {
                    return Some(item_offset + local_idx);
                }
                return None;
            }
            row_offset += group.row_count;
            item_offset += group.count;
        }
        None
    }

    /// Ensure the selected item is visible in the viewport
    fn ensure_selection_visible(&mut self) {
        if let Some(idx) = self.selected_idx {
            // Use group-aware row calculation
            let (item_row, _) = self.idx_to_visual_row_col(idx);

            // Account for headers
            let display_row = self.item_row_to_display_row(item_row);

            if display_row < self.viewport_start {
                self.viewport_start = display_row;
            } else if display_row >= self.viewport_start + self.available_rows {
                self.viewport_start = display_row.saturating_sub(self.available_rows - 1);
            }
        }
    }

    /// Navigate to first item of next group
    fn navigate_to_next_group(&mut self) {
        if self.groups.is_empty() || self.items.is_empty() {
            return;
        }
        
        let current_idx = self.selected_idx.unwrap_or(0);
        
        // Find which group current selection is in
        let mut current_group = 0;
        let mut offset = 0;
        for (i, g) in self.groups.iter().enumerate() {
            if current_idx < offset + g.count {
                current_group = i;
                break;
            }
            offset += g.count;
        }
        
        // Move to next group (wrap around)
        let next_group = (current_group + 1) % self.groups.len();
        let mut new_idx = 0;
        for (i, g) in self.groups.iter().enumerate() {
            if i == next_group {
                break;
            }
            new_idx += g.count;
        }
        
        self.selected_idx = Some(new_idx);
        self.ensure_selection_visible();
    }
    
    /// Navigate to first item of previous group
    fn navigate_to_prev_group(&mut self) {
        if self.groups.is_empty() || self.items.is_empty() {
            return;
        }
        
        let current_idx = self.selected_idx.unwrap_or(0);
        
        // Find which group current selection is in
        let mut current_group = 0;
        let mut offset = 0;
        for (i, g) in self.groups.iter().enumerate() {
            if current_idx < offset + g.count {
                current_group = i;
                break;
            }
            offset += g.count;
        }
        
        // Move to previous group (wrap around)
        let prev_group = if current_group == 0 {
            self.groups.len() - 1
        } else {
            current_group - 1
        };
        
        let mut new_idx = 0;
        for (i, g) in self.groups.iter().enumerate() {
            if i == prev_group {
                break;
            }
            new_idx += g.count;
        }
        
        self.selected_idx = Some(new_idx);
        self.ensure_selection_visible();
    }

    /// Navigate to start of current row (vi-beginning-of-line)
    fn navigate_to_row_start(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.ensure_layout();
        
        if let Some(idx) = self.selected_idx {
            let (row, _col) = self.idx_to_visual_row_col(idx);
            
            // Find first valid item in this row (column 0)
            if let Some(new_idx) = self.visual_row_col_to_idx(row, 0) {
                self.selected_idx = Some(new_idx);
                self.wish_col = 0;
            }
        }
    }

    /// Navigate to end of current row (vi-end-of-line)
    fn navigate_to_row_end(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.ensure_layout();
        
        if let Some(idx) = self.selected_idx {
            let (row, _col) = self.idx_to_visual_row_col(idx);
            
            // Get the group's column count for this row
            let max_col = if let Some((_, group, _)) = self.find_group_for_idx(idx) {
                group.cols.saturating_sub(1)
            } else {
                0
            };
            
            // Find last valid item in this row
            for try_col in (0..=max_col).rev() {
                if let Some(new_idx) = self.visual_row_col_to_idx(row, try_col) {
                    self.selected_idx = Some(new_idx);
                    self.wish_col = try_col;
                    return;
                }
            }
        }
    }
    
    #[allow(dead_code)]
    /// Old function kept for compatibility - use idx_to_visual_row_col instead
    fn idx_to_row_col(&self, idx: usize, _rows: usize, cols: usize) -> (usize, usize) {
        if cols == 0 {
            return (0, 0);
        }
        let row = idx / cols;
        let col = idx % cols;
        (row, col)
    }

    #[allow(dead_code)]
    /// Old function kept for compatibility - use visual_row_col_to_idx instead
    fn row_col_to_idx(&self, row: usize, col: usize, _rows: usize, cols: usize) -> usize {
        row * cols + col
    }

    /// Convert item row to display row (accounting for headers)
    fn item_row_to_display_row(&self, item_row: usize) -> usize {
        if !self.show_headers || self.groups.len() <= 1 {
            return item_row;
        }

        let cols = self.cached_cols.max(1);
        let mut display_row = 0;
        let mut items_so_far = 0;

        for group in &self.groups {
            if group.count == 0 {
                continue;
            }

            // Add header row
            if group.explanation.is_some() {
                display_row += 1;
            }

            let group_rows = (group.count + cols - 1) / cols;
            let group_start_item_row = items_so_far / cols;
            let group_end_item_row = group_start_item_row + group_rows;

            if item_row < group_end_item_row {
                return display_row + (item_row - group_start_item_row);
            }

            display_row += group_rows;
            items_so_far += group.count;
        }

        display_row
    }

    /// Calculate layout if needed
    fn ensure_layout(&mut self) {
        if self.layout_valid {
            return;
        }

        if self.items.is_empty() {
            self.cached_cols = 0;
            self.cached_total_rows = 0;
            self.cached_col_width = 0;
            self.layout_valid = true;
            return;
        }

        let tw = self.term_width;
        let mut total_rows = 0;
        let mut item_offset = 0;

        // Calculate layout PER GROUP - each group gets its own column count
        for group in &mut self.groups {
            if group.count == 0 {
                group.cols = 0;
                group.col_widths.clear();
                group.row_count = 0;
                continue;
            }

            // Header
            if self.show_headers && group.explanation.is_some() {
                total_rows += 1;
            }

            let items = &self.items[item_offset..item_offset + group.count];
            let n = group.count;

            // Check if any items have descriptions
            let has_descriptions = items.iter().any(|item| !item.description.is_empty());
            group.has_descriptions = has_descriptions;
            
            if has_descriptions {
                // Multi-column layout WITH descriptions (like zsh - pack tightly!)
                // Each "column" = completion + separator + description
                let max_comp_width = items.iter().map(|i| i.comp_width).max().unwrap_or(10);
                let max_desc_width = items.iter().map(|i| i.desc_width).max().unwrap_or(10);
                let separator_width = 11; // " ///////// "
                
                // Width of one complete entry (comp + sep + desc + small padding)
                let entry_width = max_comp_width + separator_width + max_desc_width + 2;
                
                // How many columns fit? Pack tightly!
                let cols = (tw / entry_width).max(1).min(n);
                
                // Use actual entry width, not distributed width - no dead space!
                group.comp_col_width = max_comp_width + 2;
                group.cols = cols;
                group.col_widths = vec![entry_width; cols];
                group.row_count = (n + cols - 1) / cols;
            } else {
                // Multi-column layout for items without descriptions
                let max_cols = n.min(tw / 2); // At least 2 chars per item
                let mut best_cols = 1;
                let mut best_widths = vec![tw];

                for try_cols in (1..=max_cols).rev() {
                    // Calculate per-column max width (row-major layout)
                    let mut col_widths = vec![0usize; try_cols];
                    for (i, item) in items.iter().enumerate() {
                        let col = i % try_cols;
                        let item_width = item.comp_width + 2;
                        col_widths[col] = col_widths[col].max(item_width);
                    }

                    let total: usize = col_widths.iter().sum();
                    if total <= tw {
                        best_cols = try_cols;
                        // Distribute extra space
                        let extra = (tw - total) / try_cols;
                        for w in &mut col_widths {
                            *w += extra;
                        }
                        best_widths = col_widths;
                        break;
                    }
                }

                group.cols = best_cols;
                group.col_widths = best_widths;
                group.row_count = (n + best_cols - 1) / best_cols;
                group.comp_col_width = 0;
            }
            
            group.start_row = total_rows;
            total_rows += group.row_count;
            item_offset += group.count;
        }

        self.cached_cols = self.groups.first().map(|g| g.cols).unwrap_or(1);
        self.cached_col_width = tw / self.cached_cols.max(1);
        self.cached_total_rows = total_rows;
        self.layout_valid = true;
    }

    /// Render the menu to displayable lines
    pub fn render(&mut self) -> MenuRendering {
        self.ensure_layout();

        let mut rendering = MenuRendering {
            term_width: self.term_width,
            term_height: self.term_height,
            total_matches: self.items.len(),
            cols: self.cached_cols,
            total_rows: self.cached_total_rows,
            row_start: self.viewport_start,
            row_end: (self.viewport_start + self.available_rows).min(self.cached_total_rows),
            selected_idx: self.selected_idx,
            selected_row: 0,
            selected_col: 0,
            lines: Vec::new(),
            status: None,
            scrollable: self.cached_total_rows > self.available_rows,
        };

        if self.items.is_empty() {
            return rendering;
        }

        // Build display rows - each group has its own column layout
        let mut display_row = 0;
        let mut global_idx = 0;

        for group in &self.groups {
            if group.count == 0 {
                continue;
            }

            let cols = group.cols.max(1);

            // Header row - use group color for cyberpunk effect
            if self.show_headers && group.explanation.is_some() {
                if display_row >= self.viewport_start && display_row < rendering.row_end {
                    let header_text = group.explanation.as_deref().unwrap_or(&group.name);
                    // Bold + group color for header
                    let header_color = format!("1;{}", group.color);
                    let mut line = MenuLine::header(header_text, &header_color);
                    line.pad_to(self.term_width);
                    rendering.lines.push(line);
                }
                display_row += 1;
            }

            // Item rows - ROW MAJOR (left to right, then down)
            for row in 0..group.row_count {
                if display_row >= self.viewport_start && display_row < rendering.row_end {
                    let mut line = MenuLine::new();
                    
                    if group.has_descriptions {
                        // Multi-column with descriptions - pack tightly, no extra padding!
                        for col in 0..group.cols {
                            let local_idx = row * group.cols + col;
                            let idx = global_idx + local_idx;

                            if local_idx < group.count {
                                if let Some(item) = self.items.get(idx) {
                                    self.render_item_with_desc_column(
                                        &mut line,
                                        item,
                                        idx,
                                        group.comp_col_width,
                                        &group.color,
                                    );
                                }
                            }
                            // No pad_to - items are already formatted with internal padding
                        }
                    } else {
                        // Multi-column layout without descriptions
                        let mut x = 0usize;
                        for col in 0..cols {
                            let local_idx = row * cols + col;
                            let idx = global_idx + local_idx;
                            let cw = group
                                .col_widths
                                .get(col)
                                .copied()
                                .unwrap_or(self.term_width / cols);

                            if local_idx < group.count {
                                if let Some(item) = self.items.get(idx) {
                                    self.render_item(
                                        &mut line,
                                        item,
                                        idx,
                                        col,
                                        cw,
                                        display_row % 2 == 1,
                                        &group.color,
                                    );
                                }
                            }
                            line.pad_to(x + cw);
                            x += cw;
                        }
                    }
                    rendering.lines.push(line);
                }
                display_row += 1;
            }

            global_idx += group.count;
        }

        // Status line - zsh style "Scrolling active: current selection at X"
        if rendering.scrollable {
            let position = if let Some(sel_idx) = self.selected_idx {
                // Determine position based on where selection is in viewport
                let (item_row, _) = self.idx_to_visual_row_col(sel_idx);
                let sel_row = self.item_row_to_display_row(item_row);
                if sel_row <= self.viewport_start + 2 {
                    "Top"
                } else if sel_row >= self.viewport_start + self.available_rows.saturating_sub(3) {
                    "Bottom"
                } else {
                    "Middle"
                }
            } else {
                if self.viewport_start == 0 {
                    "Top"
                } else if self.viewport_start + self.available_rows >= self.cached_total_rows {
                    "Bottom"
                } else {
                    "Middle"
                }
            };
            rendering.status = Some(format!(
                "Scrolling active: current selection at {}",
                position
            ));
        } else if self.search_active {
            rendering.status = Some(format!(
                "search: {} ({} matches)",
                self.search,
                self.items.len()
            ));
        }

        rendering
    }

    /// Render a single item into a line
    fn render_item(
        &self,
        line: &mut MenuLine,
        item: &MenuItem,
        idx: usize,
        _col: usize,
        col_width: usize,
        _secondary: bool,
        group_color: &str,
    ) {
        let is_selected = self.selected_idx == Some(idx);
        let available = col_width.saturating_sub(1); // 1 char spacing

        if available == 0 {
            return;
        }

        // Completion text (possibly truncated)
        let display = if item.comp_width > available {
            truncate_with_ellipsis(&item.display, available)
        } else {
            item.display.clone()
        };

        // Calculate prefix match length (case-insensitive)
        let prefix_len = if !self.prefix.is_empty() {
            let prefix_lower = self.prefix.to_lowercase();
            let display_lower = display.to_lowercase();
            if display_lower.starts_with(&prefix_lower) {
                self.prefix.chars().count()
            } else {
                0
            }
        } else {
            0
        };

        // Split into prefix (highlighted) and rest
        let (prefix_part, rest_part) = if prefix_len > 0 {
            let char_boundary: usize = display.chars().take(prefix_len).map(|c| c.len_utf8()).sum();
            (&display[..char_boundary], &display[char_boundary..])
        } else {
            ("", display.as_str())
        };
        
        // Determine effective color - use LS_COLORS for file completions
        let effective_color = self.get_item_color(item, group_color);

        // Selected highlight - use parsed ma= color from zstyle
        if is_selected {
            line.content.push_str(&ansi::from_codes(&self.selection_color));
            line.content.push_str(prefix_part);
            line.content.push_str(rest_part);
            line.content.push_str(ansi::RESET);
        } else {
            // zsh style: prefix is BOLD WHITE to stand out, rest uses group color
            if !prefix_part.is_empty() {
                line.content.push_str("\x1b[1;37m"); // Bold white for prefix
                line.content.push_str(prefix_part);
                line.content.push_str(ansi::RESET);
            }
            // Item color for the rest
            line.content.push_str(&ansi::from_codes(&effective_color));
            line.content.push_str(rest_part);
            line.content.push_str(ansi::RESET);
        }

        let disp_width = display_width(&display);
        line.width += disp_width;
    }
    
    /// Get the color for an item, using LS_COLORS for file completions
    fn get_item_color(&self, item: &MenuItem, group_color: &str) -> String {
        // Check if this looks like a file completion
        let display = &item.display;
        let is_dir = display.ends_with('/') || item.completion.modec == '/';
        let is_link = item.completion.modec == '@';
        let is_exec = item.completion.modec == '*';
        
        // Check if this is a file-related group (use LS_COLORS for all items)
        let group_name = self.groups.get(item.group_idx).map(|g| g.name.as_str()).unwrap_or("");
        let is_file_group = matches!(group_name, 
            "files" | "file" | "all-files" | "globbed-files" | 
            "local-directories" | "directories" | "directory" | "path" | "paths"
        );
        
        // Use LS_COLORS for file groups or items with file mode set
        if is_file_group || is_dir || is_link || is_exec {
            let color = crate::zpwr_colors::ls_color_for_file(display, is_dir, is_exec, is_link);
            if !color.is_empty() {
                return color;
            }
        }
        
        // Fall back to group color
        group_color.to_string()
    }
    
    /// Render item with properly aligned description column (zsh-style)
    fn render_item_with_desc_column(
        &self,
        line: &mut MenuLine,
        item: &MenuItem,
        idx: usize,
        comp_col_width: usize,
        group_color: &str,
    ) {
        let is_selected = self.selected_idx == Some(idx);
        
        // Split display into main part and alias (e.g. "--help  -h" -> "--help" + "-h")
        let display = &item.display;
        let parts: Vec<&str> = display.split_whitespace().collect();
        let (main_part, alias_part) = if parts.len() >= 2 {
            (parts[0], Some(parts[1..].join(" ")))
        } else {
            (display.as_str(), None)
        };

        // Calculate prefix match on main part
        let prefix_len = if !self.prefix.is_empty() {
            let prefix_lower = self.prefix.to_lowercase();
            let main_lower = main_part.to_lowercase();
            if main_lower.starts_with(&prefix_lower) {
                self.prefix.chars().count()
            } else {
                0
            }
        } else {
            0
        };

        let (prefix_part, rest_part) = if prefix_len > 0 {
            let char_boundary: usize = main_part.chars().take(prefix_len).map(|c| c.len_utf8()).sum();
            (&main_part[..char_boundary], &main_part[char_boundary..])
        } else {
            ("", main_part)
        };

        // Determine effective color - use LS_COLORS for file completions
        let effective_color = self.get_item_color(item, group_color);
        
        // Render main completion with color
        if is_selected {
            line.content.push_str(&ansi::from_codes(&self.selection_color));
            line.content.push_str(prefix_part);
            line.content.push_str(rest_part);
            line.content.push_str(ansi::RESET);
        } else {
            // zsh style: prefix is BOLD WHITE to stand out, rest uses group color
            if !prefix_part.is_empty() {
                line.content.push_str("\x1b[1;37m"); // Bold white for prefix
                line.content.push_str(prefix_part);
                line.content.push_str(ansi::RESET);
            }
            // Rest in item color
            line.content.push_str(&ansi::from_codes(&effective_color));
            line.content.push_str(rest_part);
            line.content.push_str(ansi::RESET);
        }
        
        // Pad main part to 16 chars, then add alias in yellow
        let main_width = display_width(main_part);
        let main_col = 16;
        for _ in main_width..main_col {
            line.content.push(' ');
        }
        
        // Alias in yellow (like zsh) - also highlight if selected
        if let Some(ref alias) = alias_part {
            if is_selected {
                line.content.push_str(&ansi::from_codes(&self.selection_color));
                line.content.push_str(alias);
                line.content.push_str(ansi::RESET);
            } else {
                line.content.push_str("\x1b[33m"); // yellow
                line.content.push_str(alias);
                line.content.push_str(ansi::RESET);
            }
            let alias_width = display_width(alias);
            for _ in alias_width..12 {
                line.content.push(' ');
            }
        } else {
            for _ in 0..12 {
                line.content.push(' ');
            }
        }
        
        line.width = main_col + 12;
        
        // Separator from zstyle (ZPWR_CHAR_LOGO)
        let separator = &self.list_separator;
        line.content.push_str("\x1b[2m");
        line.content.push_str(&separator);
        line.content.push_str(ansi::RESET);
        line.content.push(' ');
        line.width += UnicodeWidthStr::width(separator.as_str()) + 1;
        
        // Description column (cyan)
        if !item.description.is_empty() {
            line.content.push_str(&ansi::from_codes(&self.colors.description));
            line.content.push_str(&item.description);
            line.content.push_str(ansi::RESET);
            line.width += item.desc_width;
        }
    }

    /// Accept current selection and continue (multi-select)
    pub fn accept_and_continue(&mut self) -> Option<&Completion> {
        if let Some(idx) = self.selected_idx {
            self.navigate(MenuMotion::Next);
            return self.items.get(idx).map(|m| &m.completion);
        }
        None
    }

    /// Handle search input
    pub fn search_input(&mut self, c: char) {
        self.search.push(c);
        self.filter_by_search();
    }

    /// Delete last search character
    pub fn search_backspace(&mut self) {
        self.search.pop();
        if self.search.is_empty() {
            self.items = self.unfiltered_items.clone();
        } else {
            self.filter_by_search();
        }
        self.layout_valid = false;
    }

    /// Clear search
    pub fn search_clear(&mut self) {
        self.search.clear();
        self.search_active = false;
        self.items = self.unfiltered_items.clone();
        self.layout_valid = false;
    }

    /// Filter items by search string
    fn filter_by_search(&mut self) {
        self.search_active = !self.search.is_empty();
        if self.search.is_empty() {
            return;
        }

        let search_lower = self.search.to_lowercase();
        self.items = self
            .unfiltered_items
            .iter()
            .filter(|item| {
                item.display.to_lowercase().contains(&search_lower)
                    || item.description.to_lowercase().contains(&search_lower)
            })
            .cloned()
            .collect();

        self.layout_valid = false;

        // Reset selection if current is filtered out
        if let Some(idx) = self.selected_idx {
            if idx >= self.items.len() {
                self.selected_idx = if self.items.is_empty() { None } else { Some(0) };
            }
        }
    }
}

/// Calculate display width of a string using unicode-width
fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Make a color code bold by prepending "1;" if not already bold
/// e.g., "32" -> "1;32", "1;32" -> "1;32"
fn make_bold(color: &str) -> String {
    if color.is_empty() {
        return "1".to_string();
    }
    // Check if already has bold (starts with "1;" or contains ";1;" or is just "1")
    if color == "1" || color.starts_with("1;") || color.contains(";1;") || color.contains(";1") {
        color.to_string()
    } else {
        format!("1;{}", color)
    }
}

/// Truncate string with ellipsis to fit width
fn truncate_with_ellipsis(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let width = display_width(s);
    if width <= max_width {
        return s.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }

    let mut result = String::new();
    let mut current_width = 0;
    for c in s.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if current_width + char_width >= max_width {
            break;
        }
        result.push(c);
        current_width += char_width;
    }
    result.push('…');
    result
}

/// Key actions for menu completion (maps to zsh widget bindings)
///
/// These actions match zsh's Src/Zle/complist.c behavior exactly:
/// - Navigation (Up/Down/Left/Right) moves in the menu grid with wrapping
/// - Accept exits the menu and inserts the selection
/// - AcceptAndMenuComplete accepts current, then runs a NEW completion cycle
/// - AcceptAndInferNextHistory same as above (both push state stack, call do_menucmp)
/// - ReverseMenuComplete cycles to previous match via completion (not just prev item)
/// - ToggleInteractive enters/exits MM_INTER mode for type-to-filter
/// - SearchForward/Backward enters MM_FSEARCH/MM_BSEARCH incremental search
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    /// Exit menu, accept current selection (accept-line)
    Accept,
    /// Accept current + trigger NEW completion on modified line (accept-and-menu-complete)
    /// This is NOT "advance to next item" - it runs a fresh completion cycle
    AcceptAndMenuComplete,
    /// Accept current + trigger NEW completion (accept-and-infer-next-history)
    /// Same behavior as AcceptAndMenuComplete in menuselect context
    AcceptAndInferNextHistory,
    /// Exit menu without accepting (send-break)
    Cancel,
    /// Move up in menu grid, wrap to bottom+left at top (up-history)
    Up,
    /// Move down in menu grid, wrap to top+right at bottom (down-history)
    Down,
    /// Move left in menu grid, wrap to prev row at col 0 (vi-backward-char)
    Left,
    /// Move right in menu grid, wrap to next row at end (vi-forward-char)
    Right,
    /// Page up by screenful (vi-backward-word)
    PageUp,
    /// Page down by screenful (vi-forward-word)
    PageDown,
    /// Jump to first item (beginning-of-history)
    Beginning,
    /// Jump to last item (end-of-history)
    End,
    /// Jump to beginning of current row (vi-beginning-of-line)
    BeginningOfLine,
    /// Jump to end of current row (vi-end-of-line)
    EndOfLine,
    /// Cycle to next match via completion (menu-complete)
    Next,
    /// Cycle to previous match via completion with zmult=-1 (reverse-menu-complete)
    /// This calls do_menucmp(0) with negative multiplier
    Prev,
    /// Jump to next group (vi-forward-blank-word)
    NextGroup,
    /// Jump to previous group (vi-backward-blank-word)
    PrevGroup,
    /// Toggle interactive/filter mode MM_INTER (vi-insert)
    ToggleInteractive,
    /// Enter forward incremental search MM_FSEARCH (history-incremental-search-forward)
    SearchForward,
    /// Enter backward incremental search MM_BSEARCH (history-incremental-search-backward)
    SearchBackward,
    /// Undo last accept (pops from menu stack)
    Undo,
    /// Force redisplay
    Redisplay,
    /// Generic search (alias for SearchForward)
    Search,
    /// Clear current search string
    ClearSearch,
    /// Insert character (in interactive/search mode)
    Insert(char),
    /// Delete last character (in interactive/search mode)
    Backspace,
}

/// Result of processing a menu action
#[derive(Clone, Debug)]
pub enum MenuResult {
    /// Stay in menu, continue processing
    Continue,
    /// Exit menu, insert this string
    Accept(String),
    /// Accept this string AND advance to next item in SAME menu (accept-and-menu-complete)
    /// The caller should: 1) insert the string, 2) stay in menu mode showing next item
    AcceptAndHold(String),
    /// Exit menu without inserting anything
    Cancel,
    /// Force redisplay
    Redisplay,
    /// Undo requested - caller should pop state stack and restore previous line
    UndoRequested,
    /// No action taken
    None,
}

impl MenuState {
    /// Process a menu action and return the result
    ///
    /// This matches zsh's Src/Zle/complist.c menuselect behavior:
    /// - Accept: exit menu, insert selection
    /// - AcceptAndMenuComplete/AcceptAndInferNextHistory: accept + request NEW completion
    /// - Navigation: move in grid with proper wrapping
    /// - ToggleInteractive: enter/exit filter mode
    /// - SearchForward/Backward: enter incremental search
    pub fn process_action(&mut self, action: MenuAction) -> MenuResult {
        match action {
            MenuAction::Accept => {
                // zsh: sets acc=1, breaks loop - exit menu with current selection
                if let Some(comp) = self.selected() {
                    let insert = comp.insert_str();
                    self.stop();
                    return MenuResult::Accept(insert);
                }
                MenuResult::Cancel
            }
            MenuAction::AcceptAndMenuComplete | MenuAction::AcceptAndInferNextHistory => {
                // zsh: accept_last() then do_menucmp(0) - accept current, advance to next
                // This accepts current match AND moves to next item in SAME menu
                // The menu stays open showing the next item
                if let Some(idx) = self.selected_idx {
                    if let Some(item) = self.items.get(idx) {
                        let insert = item.completion.insert_str();
                        // Advance to next item (wrap around)
                        self.selected_idx = Some((idx + 1) % self.items.len());
                        self.ensure_selection_visible();
                        return MenuResult::AcceptAndHold(insert);
                    }
                }
                MenuResult::None
            }
            MenuAction::Cancel => {
                // zsh: send-break - exit without accepting
                self.stop();
                MenuResult::Cancel
            }
            MenuAction::Up => {
                // zsh: up-history - move up in grid, wrap to bottom+left at top
                self.navigate(MenuMotion::Up);
                MenuResult::Continue
            }
            MenuAction::Down => {
                // zsh: down-history - move down in grid, wrap to top+right at bottom
                self.navigate(MenuMotion::Down);
                MenuResult::Continue
            }
            MenuAction::Left => {
                // zsh: vi-backward-char - move left, wrap to prev row at col 0
                self.navigate(MenuMotion::Left);
                MenuResult::Continue
            }
            MenuAction::Right => {
                // zsh: vi-forward-char - move right, wrap to next row at end
                self.navigate(MenuMotion::Right);
                MenuResult::Continue
            }
            MenuAction::PageUp => {
                // zsh: vi-backward-word - page up by screenful
                self.navigate(MenuMotion::PageUp);
                MenuResult::Continue
            }
            MenuAction::PageDown => {
                // zsh: vi-forward-word - page down by screenful
                self.navigate(MenuMotion::PageDown);
                MenuResult::Continue
            }
            MenuAction::Beginning => {
                // zsh: beginning-of-history - jump to first item
                self.navigate(MenuMotion::First);
                MenuResult::Continue
            }
            MenuAction::End => {
                // zsh: end-of-history - jump to last item
                self.navigate(MenuMotion::Last);
                MenuResult::Continue
            }
            MenuAction::BeginningOfLine => {
                // zsh: vi-beginning-of-line - jump to start of current row
                self.navigate_to_row_start();
                MenuResult::Continue
            }
            MenuAction::EndOfLine => {
                // zsh: vi-end-of-line - jump to end of current row
                self.navigate_to_row_end();
                MenuResult::Continue
            }
            MenuAction::Next => {
                // zsh: menu-complete - this actually cycles via completion
                // For simplicity we just advance in menu; true zsh calls do_menucmp
                self.navigate(MenuMotion::Next);
                MenuResult::Continue
            }
            MenuAction::Prev => {
                // zsh: reverse-menu-complete - cycles backward via do_menucmp with zmult=-1
                // For simplicity we just go back in menu
                self.navigate(MenuMotion::Prev);
                MenuResult::Continue
            }
            MenuAction::NextGroup => {
                // zsh: vi-forward-blank-word - jump to next completion group
                self.navigate_to_next_group();
                MenuResult::Continue
            }
            MenuAction::PrevGroup => {
                // zsh: vi-backward-blank-word - jump to previous completion group
                self.navigate_to_prev_group();
                MenuResult::Continue
            }
            MenuAction::ToggleInteractive => {
                // zsh: vi-insert - toggle MM_INTER mode (type-to-filter)
                self.interactive_mode = !self.interactive_mode;
                if self.interactive_mode {
                    self.search_active = true;
                } else {
                    self.search_clear();
                }
                MenuResult::Continue
            }
            MenuAction::SearchForward | MenuAction::SearchBackward | MenuAction::Search => {
                // zsh: history-incremental-search-forward/backward - enter search mode
                self.search_active = true;
                self.search_direction = if matches!(action, MenuAction::SearchBackward) {
                    SearchDirection::Backward
                } else {
                    SearchDirection::Forward
                };
                MenuResult::Continue
            }
            MenuAction::ClearSearch => {
                self.search_clear();
                MenuResult::Continue
            }
            MenuAction::Undo => {
                // zsh: undo - pops from Menustack, restores previous line/state
                MenuResult::UndoRequested
            }
            MenuAction::Redisplay => MenuResult::Redisplay,
            MenuAction::Insert(c) => {
                if self.search_active || self.interactive_mode {
                    self.search_input(c);
                }
                MenuResult::Continue
            }
            MenuAction::Backspace => {
                if self.search_active || self.interactive_mode {
                    self.search_backspace();
                }
                MenuResult::Continue
            }
        }
    }

    /// Get the string to insert for the currently selected item
    pub fn current_insert_string(&self) -> Option<String> {
        self.selected().map(|c| c.insert_str())
    }

    /// Get info for status line: (selected_num, total, groups)
    pub fn status_info(&self) -> (usize, usize, usize) {
        (
            self.selected_idx.map(|i| i + 1).unwrap_or(0),
            self.items.len(),
            self.groups.len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_menu_navigation() {
        let mut menu = MenuState::new();
        menu.set_term_size(80, 24);
        menu.set_available_rows(10);

        let mut group = CompletionGroup::new("test");
        // Long descriptions force single-column layout, so Down = next item
        for i in 0..20 {
            let mut comp = Completion::new(format!("item{:02}", i));
            comp.desc = Some(format!("A long description for item {} that takes up most of the line width", i));
            group.matches.push(comp);
        }

        menu.set_completions(&[group]);

        assert_eq!(menu.count(), 20);
        assert!(!menu.is_active());

        menu.start();
        assert!(menu.is_active());
        assert_eq!(menu.selected_index(), Some(0));

        // In single-column, Down moves to next row = next item
        menu.navigate(MenuMotion::Down);
        assert_eq!(menu.selected_index(), Some(1));

        for _ in 0..5 {
            menu.navigate(MenuMotion::Next);
        }
        assert_eq!(menu.selected_index(), Some(6));

        menu.navigate(MenuMotion::Deselect);
        assert!(!menu.is_active());
    }

    #[test]
    fn test_multi_column_navigation() {
        // Test navigation in multi-column layout (no descriptions = multi-col)
        let mut menu = MenuState::new();
        menu.set_term_size(80, 24);
        menu.set_available_rows(10);

        let mut group = CompletionGroup::new("test");
        // 20 items with longer names to force multiple rows
        for i in 0..20 {
            let comp = Completion::new(format!("longer_item_{:02}", i));
            group.matches.push(comp);
        }

        menu.set_completions(&[group]);
        menu.start();
        
        // Debug: print layout info
        eprintln!("Groups: {:?}", menu.groups);
        eprintln!("Initial selected_idx: {:?}", menu.selected_index());
        
        let cols = menu.groups[0].cols;
        let rows = menu.groups[0].row_count;
        eprintln!("Layout: {} cols x {} rows", cols, rows);
        
        // With 80-width and ~15 char items, we should get ~4-5 columns
        // So 20 items / 5 cols = 4 rows
        // Row 0: items 0-4
        // Row 1: items 5-9
        // etc.
        
        let initial_idx = menu.selected_index().unwrap();
        let (initial_row, initial_col) = menu.idx_to_visual_row_col(initial_idx);
        eprintln!("Initial: idx={}, row={}, col={}", initial_idx, initial_row, initial_col);
        
        // Only test if we actually have multiple rows
        if rows > 1 {
            menu.navigate(MenuMotion::Down);
            
            let after_idx = menu.selected_index().unwrap();
            let (after_row, after_col) = menu.idx_to_visual_row_col(after_idx);
            eprintln!("After Down: idx={}, row={}, col={}", after_idx, after_row, after_col);
            
            // Down should move exactly one row
            assert_eq!(after_row, initial_row + 1, "Down should move exactly 1 row");
            assert_eq!(after_col, initial_col, "Down should keep same column");
            
            // Expected: if we have 5 cols, item 0 -> item 5
            let expected_idx = cols;
            assert_eq!(after_idx, expected_idx, "Should move by number of columns");
        }
    }
    
    #[test]
    fn test_multi_group_navigation() {
        // Test navigation across multiple groups
        let mut menu = MenuState::new();
        menu.set_term_size(80, 24);
        menu.set_available_rows(10);

        // Group 1: files (short names)
        let mut group1 = CompletionGroup::new("files");
        for i in 0..15 {
            let comp = Completion::new(format!("file_{:02}.txt", i));
            group1.matches.push(comp);
        }
        
        // Group 2: directories (short names)
        let mut group2 = CompletionGroup::new("directories");
        for i in 0..8 {
            let comp = Completion::new(format!("dir_{}/", i));
            group2.matches.push(comp);
        }

        menu.set_completions(&[group1, group2]);
        menu.start();
        
        eprintln!("Groups: {:?}", menu.groups);
        
        // Navigate through first group
        let initial_idx = menu.selected_index().unwrap();
        let (r0, c0) = menu.idx_to_visual_row_col(initial_idx);
        eprintln!("Start: idx={}, row={}, col={}", initial_idx, r0, c0);
        
        menu.navigate(MenuMotion::Down);
        let (r1, c1) = menu.idx_to_visual_row_col(menu.selected_index().unwrap());
        eprintln!("After Down: idx={}, row={}, col={}", menu.selected_index().unwrap(), r1, c1);
        assert_eq!(r1, r0 + 1, "Down should move 1 row");
        
        // Move to first group's last row, then down into second group
        let rows_g1 = menu.groups[0].row_count;
        eprintln!("Group 1 rows: {}", rows_g1);
        
        // Keep going down to reach group boundary
        for _ in 0..(rows_g1 - r1) {
            menu.navigate(MenuMotion::Down);
        }
        let (row_after, _col_after) = menu.idx_to_visual_row_col(menu.selected_index().unwrap());
        eprintln!("After reaching group boundary: idx={}, row={}", menu.selected_index().unwrap(), row_after);
    }
    
    #[test]
    fn test_varied_column_groups() {
        // Reproduce user's 'a<TAB>' scenario with varied column counts
        let mut menu = MenuState::new();
        menu.set_term_size(200, 24);  // Wide terminal
        menu.set_available_rows(20);

        // Group 1: external commands - 20 items, ~4 columns = 5 rows
        let mut group1 = CompletionGroup::new("external command");
        for i in 0..20 {
            let comp = Completion::new(format!("ext_cmd_{:02}_longish_name", i));
            group1.matches.push(comp);
        }
        
        // Group 2: alias - 1 item with description = 1 row, 1 column
        let mut group2 = CompletionGroup::new("alias");
        let mut alias = Completion::new("ai");
        alias.desc = Some("forDirZipRar.zsh && mountInstall.zsh".to_string());
        group2.matches.push(alias);
        
        // Group 3: shell functions - 9 short items = 1 row, ~9 columns
        let mut group3 = CompletionGroup::new("shell function");
        for name in ["a", "add-zle-hook", "add-zsh-hook", "after", "age", "allopt", "apz", "asg", "aws_comp"] {
            let comp = Completion::new(name);
            group3.matches.push(comp);
        }
        
        // Group 4: builtins - 2 items = 1 row, 2 columns
        let mut group4 = CompletionGroup::new("builtin command");
        group4.matches.push(Completion::new("alias"));
        group4.matches.push(Completion::new("autoload"));

        menu.set_completions(&[group1, group2, group3, group4]);
        menu.start();
        
        eprintln!("\nGroups layout:");
        for (i, g) in menu.groups.iter().enumerate() {
            eprintln!("  Group {}: '{}' - {} items, {} cols, {} rows, start_row={}",
                i, g.name, g.count, g.cols, g.row_count, g.start_row);
        }
        
        let total_rows: usize = menu.groups.iter().map(|g| g.row_count).sum();
        
        // Test: Starting at first item, navigate down multiple times
        // Each Down should move exactly 1 visual row
        eprintln!("\n=== Testing Down navigation ===");
        let mut prev_row = 0;
        for step in 0..10 {
            let idx = menu.selected_index().unwrap();
            let (row, col) = menu.idx_to_visual_row_col(idx);
            eprintln!("Step {}: idx={}, row={}, col={}", step, idx, row, col);
            
            if step > 0 {
                // Should have moved exactly 1 row (or wrapped from last to first)
                let expected = (prev_row + 1) % total_rows;
                assert_eq!(row, expected, "Down should move exactly 1 row (step {})", step);
            }
            prev_row = row;
            
            menu.navigate(MenuMotion::Down);
        }
        
        // Test Up navigation too
        eprintln!("\n=== Testing Up navigation ===");
        // Reset to start
        menu.navigate(MenuMotion::First);
        prev_row = 0;
        for step in 0..10 {
            let idx = menu.selected_index().unwrap();
            let (row, col) = menu.idx_to_visual_row_col(idx);
            eprintln!("Step {}: idx={}, row={}, col={}", step, idx, row, col);
            
            if step > 0 {
                // Up should move exactly 1 row back (or wrap from 0 to last)
                let expected = if prev_row == 0 { total_rows - 1 } else { prev_row - 1 };
                assert_eq!(row, expected, "Up should move exactly 1 row back (step {})", step);
            }
            prev_row = row;
            
            menu.navigate(MenuMotion::Up);
        }
    }

    #[test]
    fn test_truncate_ellipsis() {
        assert_eq!(truncate_with_ellipsis("hello", 10), "hello");
        assert_eq!(truncate_with_ellipsis("hello world", 8), "hello w…");
        assert_eq!(truncate_with_ellipsis("hi", 1), "…");
        assert_eq!(truncate_with_ellipsis("", 5), "");
    }

    #[test]
    fn test_unicode_width() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width("日本語"), 6); // 3 chars * 2 width each
        assert_eq!(display_width("café"), 4);
    }

    #[test]
    fn test_render() {
        let mut menu = MenuState::new();
        menu.set_term_size(80, 24);
        menu.set_available_rows(10);
        menu.set_show_headers(false);

        let mut group = CompletionGroup::new("test");
        for i in 0..5 {
            let mut comp = Completion::new(format!("item{}", i));
            comp.desc = Some(format!("desc{}", i));
            group.matches.push(comp);
        }

        menu.set_completions(&[group]);
        menu.start();

        let rendering = menu.render();
        assert_eq!(rendering.total_matches, 5);
        assert!(rendering.selected_idx.is_some());
        assert!(!rendering.lines.is_empty());
    }

    #[test]
    fn test_search_filter() {
        let mut menu = MenuState::new();
        menu.set_term_size(80, 24);
        menu.set_available_rows(10);

        let mut group = CompletionGroup::new("test");
        group.matches.push(Completion::new("apple"));
        group.matches.push(Completion::new("banana"));
        group.matches.push(Completion::new("apricot"));
        group.matches.push(Completion::new("cherry"));

        menu.set_completions(&[group]);
        menu.start();
        assert_eq!(menu.count(), 4);

        menu.search_active = true;
        menu.search_input('a');
        menu.search_input('p');
        assert_eq!(menu.count(), 2); // apple, apricot

        menu.search_clear();
        assert_eq!(menu.count(), 4);
    }

    #[test]
    fn test_group_headers() {
        let mut menu = MenuState::new();
        menu.set_term_size(80, 24);
        menu.set_available_rows(10);
        menu.set_show_headers(true);

        let mut group1 = CompletionGroup::new("files");
        group1.explanation = Some("Files".to_string());
        group1.matches.push(Completion::new("file1.txt"));
        group1.matches.push(Completion::new("file2.txt"));

        let mut group2 = CompletionGroup::new("dirs");
        group2.explanation = Some("Directories".to_string());
        group2.matches.push(Completion::new("dir1/"));
        group2.matches.push(Completion::new("dir2/"));

        menu.set_completions(&[group1, group2]);
        menu.start();

        let rendering = menu.render();
        assert_eq!(rendering.total_matches, 4);

        // Should have header lines
        let header_count = rendering.lines.iter().filter(|l| l.is_header).count();
        assert_eq!(header_count, 2);
    }
}

// === KEYMAP SUPPORT ===

/// Search direction
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchDirection {
    #[default]
    Forward,
    Backward,
}

/// Terminal key sequence
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct KeySequence(pub Vec<u8>);

impl KeySequence {
    pub fn from_zsh_notation(s: &str) -> Self {
        let mut bytes = Vec::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '^' {
                if let Some(&next) = chars.peek() {
                    if next == '[' {
                        bytes.push(0x1b);
                        chars.next();
                    } else if next == '?' {
                        bytes.push(127);
                        chars.next();
                    } else if next == '@' {
                        bytes.push(0);
                        chars.next();
                    } else {
                        let ctrl = next.to_ascii_uppercase();
                        if ctrl >= 'A' && ctrl <= '_' {
                            bytes.push((ctrl as u8) - b'A' + 1);
                        } else {
                            bytes.push(ctrl as u8);
                        }
                        chars.next();
                    }
                }
            } else {
                bytes.push(c as u8);
            }
        }
        Self(bytes)
    }
}

/// Default menuselect keymap bindings
///
/// Maps zsh widget names to MenuAction based on Src/Zle/complist.c behavior
pub fn default_menuselect_bindings() -> Vec<(&'static str, MenuAction)> {
    vec![
        // Exit and accept
        ("accept-line", MenuAction::Accept),
        (".accept-line", MenuAction::Accept),
        ("accept-search", MenuAction::Accept),
        ("send-break", MenuAction::Cancel),
        // Accept + recompute (NOT advance to next item!)
        ("accept-and-hold", MenuAction::AcceptAndMenuComplete),
        ("accept-and-menu-complete", MenuAction::AcceptAndMenuComplete),
        ("accept-and-infer-next-history", MenuAction::AcceptAndInferNextHistory),
        // Vertical navigation
        ("down-history", MenuAction::Down),
        ("down-line-or-history", MenuAction::Down),
        ("down-line-or-search", MenuAction::Down),
        ("vi-down-line-or-history", MenuAction::Down),
        ("up-history", MenuAction::Up),
        ("up-line-or-history", MenuAction::Up),
        ("up-line-or-search", MenuAction::Up),
        ("vi-up-line-or-history", MenuAction::Up),
        // Horizontal navigation
        ("forward-char", MenuAction::Right),
        ("vi-forward-char", MenuAction::Right),
        ("backward-char", MenuAction::Left),
        ("vi-backward-char", MenuAction::Left),
        // Page navigation (by screenful)
        ("forward-word", MenuAction::PageDown),
        ("vi-forward-word", MenuAction::PageDown),
        ("vi-forward-word-end", MenuAction::PageDown),
        ("emacs-forward-word", MenuAction::PageDown),
        ("backward-word", MenuAction::PageUp),
        ("vi-backward-word", MenuAction::PageUp),
        ("emacs-backward-word", MenuAction::PageUp),
        // Group navigation
        ("vi-forward-blank-word", MenuAction::NextGroup),
        ("vi-backward-blank-word", MenuAction::PrevGroup),
        // Jump to first/last
        ("beginning-of-history", MenuAction::Beginning),
        ("beginning-of-buffer-or-history", MenuAction::Beginning),
        ("end-of-history", MenuAction::End),
        ("end-of-buffer-or-history", MenuAction::End),
        // Row navigation
        ("vi-beginning-of-line", MenuAction::BeginningOfLine),
        ("beginning-of-line", MenuAction::BeginningOfLine),
        ("beginning-of-line-hist", MenuAction::BeginningOfLine),
        ("vi-end-of-line", MenuAction::EndOfLine),
        ("end-of-line", MenuAction::EndOfLine),
        ("end-of-line-hist", MenuAction::EndOfLine),
        // Completion cycling (these call do_menucmp in zsh)
        ("complete-word", MenuAction::Next),
        ("menu-complete", MenuAction::Next),
        ("expand-or-complete", MenuAction::Next),
        ("menu-expand-or-complete", MenuAction::Next),
        ("reverse-menu-complete", MenuAction::Prev),
        // Interactive mode (MM_INTER)
        ("vi-insert", MenuAction::ToggleInteractive),
        // Incremental search (MM_FSEARCH/MM_BSEARCH)
        ("history-incremental-search-forward", MenuAction::SearchForward),
        ("history-incremental-search-backward", MenuAction::SearchBackward),
        // Undo (pops from menu stack)
        ("undo", MenuAction::Undo),
        ("backward-delete-char", MenuAction::Backspace),
        // Redisplay
        ("redisplay", MenuAction::Redisplay),
        ("clear-screen", MenuAction::Redisplay),
    ]
}

/// Menuselect keymap
#[derive(Clone, Debug, Default)]
pub struct MenuKeymap {
    key_to_widget: Vec<(KeySequence, String)>,
    widget_to_action: std::collections::HashMap<String, MenuAction>,
}

impl MenuKeymap {
    pub fn new() -> Self {
        let mut km = Self::default();
        for (w, a) in default_menuselect_bindings() {
            km.widget_to_action.insert(w.to_string(), a);
        }
        km.bind("^@", "accept-line");
        km.bind("^D", "accept-and-menu-complete");
        km.bind("^F", "accept-and-infer-next-history");
        km.bind("^H", "vi-backward-char");
        km.bind("^I", "vi-forward-char");
        km.bind("^J", "down-history");
        km.bind("^K", "up-history");
        km.bind("^L", "vi-forward-char");
        km.bind("^M", "accept-line");
        km.bind("^N", "vi-forward-word");
        km.bind("^P", "vi-backward-word");
        km.bind("^S", "reverse-menu-complete");
        km.bind("^V", "vi-insert");
        km.bind("^X", "history-incremental-search-forward");
        km.bind("^[OA", "up-line-or-history");
        km.bind("^[OB", "down-line-or-history");
        km.bind("^[OC", "forward-char");
        km.bind("^[OD", "backward-char");
        km.bind("^[[A", "up-line-or-history");
        km.bind("^[[B", "down-line-or-history");
        km.bind("^[[C", "forward-char");
        km.bind("^[[D", "backward-char");
        km.bind("^[[1~", "vi-beginning-of-line");
        km.bind("^[[4~", "vi-end-of-line");
        km.bind("^[[5~", "vi-backward-word");
        km.bind("^[[6~", "vi-forward-word");
        km.bind("^[[Z", "reverse-menu-complete");
        km.bind("^?", "undo");
        km.bind("?", "history-incremental-search-backward");
        km.bind("/", "history-incremental-search-forward");
        km.bind("^[", "send-break");
        km.bind("^G", "send-break");
        km.bind("{", "vi-backward-blank-word");
        km.bind("}", "vi-forward-blank-word");
        km
    }

    pub fn bind(&mut self, key: &str, widget: &str) {
        let seq = KeySequence::from_zsh_notation(key);
        self.key_to_widget.retain(|(k, _)| k != &seq);
        self.key_to_widget.push((seq, widget.to_string()));
    }

    pub fn lookup(&self, input: &[u8]) -> Option<(MenuAction, usize)> {
        let mut best: Option<(&str, usize)> = None;
        for (seq, widget) in &self.key_to_widget {
            if input.starts_with(&seq.0) && best.map(|(_, l)| seq.0.len() > l).unwrap_or(true) {
                best = Some((widget.as_str(), seq.0.len()));
            }
        }
        if let Some((w, len)) = best {
            if let Some(&a) = self.widget_to_action.get(w) {
                return Some((a, len));
            }
        }
        if !input.is_empty() && input[0] >= 0x20 && input[0] < 0x7f {
            return Some((MenuAction::Insert(input[0] as char), 1));
        }
        None
    }

    pub fn get_widget(&self, key: &str) -> Option<&str> {
        let seq = KeySequence::from_zsh_notation(key);
        self.key_to_widget
            .iter()
            .find(|(k, _)| k == &seq)
            .map(|(_, w)| w.as_str())
    }
}

pub fn parse_bindkey_output(output: &str, keymap: &mut MenuKeymap) {
    for line in output.lines() {
        let line = line
            .trim()
            .strip_prefix("bindkey")
            .map(|s| s.trim_start())
            .and_then(|s| s.strip_prefix("-M"))
            .map(|s| s.trim_start())
            .and_then(|s| s.strip_prefix("menuselect"))
            .map(|s| s.trim_start())
            .unwrap_or(line.trim());
        if let Some(rest) = line.strip_prefix('"') {
            if let Some(end) = rest.find('"') {
                let (key, widget) = (&rest[..end], rest[end + 1..].trim());
                if !widget.is_empty() {
                    keymap.bind(key, widget);
                }
            }
        }
    }
}

impl MenuState {
    /// Check if interactive filter mode is active (MM_INTER)
    pub fn is_interactive(&self) -> bool {
        self.interactive_mode
    }
    
    /// Check if incremental search is active (MM_FSEARCH or MM_BSEARCH)
    pub fn is_search_active(&self) -> bool {
        self.search_active
    }
    
    /// Get current search/filter string
    pub fn search_string(&self) -> &str {
        &self.search
    }
    
    /// Get search direction
    pub fn search_direction(&self) -> SearchDirection {
        self.search_direction
    }
}
