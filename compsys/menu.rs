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
#[derive(Clone, Debug)]
pub struct MenuState {
    /// All completion items, flattened
    items: Vec<MenuItem>,
    /// Group layouts
    groups: Vec<GroupLayout>,
    /// Current selection index (None = not in menu mode)
    selected_idx: Option<usize>,
    /// Column memory for vertical navigation
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
    /// Search/filter string
    search: String,
    /// Whether search is active
    search_active: bool,
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
}

impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
}

impl MenuState {
    pub fn new() -> Self {
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
            group_colors: std::collections::HashMap::new(),
            prefix: String::new(),
            search: String::new(),
            search_active: false,
            unfiltered_items: Vec::new(),
            layout_valid: false,
            cached_total_rows: 0,
            cached_cols: 0,
            cached_col_width: 0,
            show_headers: true,
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
                    let (row, col) = self.idx_to_row_col(idx, rows, cols);
                    let (new_row, new_col) = match motion {
                        MenuMotion::Up => {
                            if row > 0 {
                                (row - 1, self.wish_col.min(cols - 1))
                            } else {
                                let new_col = if col > 0 { col - 1 } else { cols - 1 };
                                (rows.saturating_sub(1), new_col)
                            }
                        }
                        MenuMotion::Down => {
                            if row + 1 < rows {
                                (row + 1, self.wish_col.min(cols - 1))
                            } else {
                                let new_col = (col + 1) % cols;
                                (0, new_col)
                            }
                        }
                        MenuMotion::Left => {
                            let new_col = if col > 0 { col - 1 } else { cols - 1 };
                            self.wish_col = new_col;
                            (row, new_col)
                        }
                        MenuMotion::Right => {
                            let new_col = (col + 1) % cols;
                            self.wish_col = new_col;
                            (row, new_col)
                        }
                        MenuMotion::PageUp => {
                            let page = self.available_rows.saturating_sub(1);
                            (row.saturating_sub(page), col)
                        }
                        MenuMotion::PageDown => {
                            let page = self.available_rows.saturating_sub(1);
                            ((row + page).min(rows.saturating_sub(1)), col)
                        }
                        _ => (row, col),
                    };

                    let new_idx = self.row_col_to_idx(new_row, new_col, rows, cols);
                    if new_idx < self.items.len() {
                        self.selected_idx = Some(new_idx);
                    } else {
                        // Clamp to last item in column
                        let last_row = (self.items.len().saturating_sub(1)) % rows;
                        let clamped =
                            self.row_col_to_idx(last_row.min(new_row), new_col, rows, cols);
                        if clamped < self.items.len() {
                            self.selected_idx = Some(clamped);
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
        let cols = self.cached_cols.max(1);
        (self.items.len() + cols - 1) / cols
    }

    /// Ensure the selected item is visible in the viewport
    fn ensure_selection_visible(&mut self) {
        if let Some(idx) = self.selected_idx {
            let rows = self.rows_for_items();
            let cols = self.cached_cols.max(1);
            let (item_row, _) = self.idx_to_row_col(idx, rows, cols);

            // Account for headers
            let display_row = self.item_row_to_display_row(item_row);

            if display_row < self.viewport_start {
                self.viewport_start = display_row;
            } else if display_row >= self.viewport_start + self.available_rows {
                self.viewport_start = display_row.saturating_sub(self.available_rows - 1);
            }
        }
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

    /// Convert flat index to (row, col) in column-major order
    fn idx_to_row_col(&self, idx: usize, rows: usize, _cols: usize) -> (usize, usize) {
        if rows == 0 {
            return (0, 0);
        }
        let col = idx / rows;
        let row = idx % rows;
        (row, col)
    }

    /// Convert (row, col) to flat index in column-major order
    fn row_col_to_idx(&self, row: usize, col: usize, rows: usize, _cols: usize) -> usize {
        col * rows + row
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

            // Try to fit as many columns as possible
            let max_cols = n.min(tw / 2); // At least 2 chars per item
            let mut best_cols = 1;
            let mut best_widths = vec![tw];

            // Check if any items have descriptions
            let has_descriptions = items.iter().any(|item| !item.description.is_empty());

            // If items have descriptions, use single column to show them properly
            // This matches zsh behavior for _describe completions
            let effective_max_cols = if has_descriptions {
                1 // Single column when descriptions present - like zsh
            } else {
                max_cols
            };

            for try_cols in (1..=effective_max_cols).rev() {
                // Calculate per-column max width (row-major layout)
                let mut col_widths = vec![0usize; try_cols];
                for (i, item) in items.iter().enumerate() {
                    let col = i % try_cols;
                    // Include description width for layout calculation
                    let item_width = if has_descriptions && !item.description.is_empty() {
                        item.comp_width + 4 + item.desc_width.min(30) // " -- " + desc (max 30)
                    } else {
                        item.comp_width + 2
                    };
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
                    rendering.lines.push(line);
                }
                display_row += 1;
            }

            global_idx += group.count;
        }

        // Status line
        if rendering.scrollable {
            rendering.status = Some(format!(
                "rows {}-{} of {} ({} matches)",
                rendering.row_start + 1,
                rendering.row_end,
                self.cached_total_rows,
                self.items.len()
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

        // Selected highlight
        if is_selected {
            line.content.push_str("\x1b[7m"); // Reverse video
            line.content.push_str(prefix_part);
            line.content.push_str(rest_part);
            line.content.push_str("\x1b[0m");
        } else {
            // zstyle format: =(#b)(*)=PREFIX_COLOR=REST_COLOR
            // PREFIX_COLOR is typically 1;30 (bold dark gray)
            // REST_COLOR is the group-specific color
            if !prefix_part.is_empty() {
                line.content.push_str("\x1b[1;30m"); // Bold dark gray for prefix
                line.content.push_str(prefix_part);
                line.content.push_str(ansi::RESET);
            }
            // Group color for the rest
            line.content.push_str(&ansi::from_codes(group_color));
            line.content.push_str(rest_part);
            line.content.push_str(ansi::RESET);
        }

        line.width += display_width(&display);

        // Always add description if present - use ZPWR separator style
        if !item.description.is_empty() {
            let separator = std::env::var("ZPWR_CHAR_LOGO").unwrap_or_else(|_| " -- ".to_string());
            let sep_width = UnicodeWidthStr::width(separator.as_str());
            let desc_space = available.saturating_sub(display_width(&display) + sep_width);

            // Dim separator, cyan description
            line.content.push_str("\x1b[2m"); // dim
            line.content.push_str(&separator);
            line.content.push_str(ansi::RESET);

            line.content
                .push_str(&ansi::from_codes(&self.colors.description));
            let desc = if desc_space > 0 && item.desc_width > desc_space {
                truncate_with_ellipsis(&item.description, desc_space.max(3))
            } else {
                item.description.clone()
            };
            line.content.push_str(&desc);
            line.content.push_str(ansi::RESET);
            line.width += sep_width + display_width(&desc);
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    Accept,
    AcceptAndHold,
    AcceptAndInferNext,
    Cancel,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Beginning,
    End,
    BeginningOfLine,
    EndOfLine,
    Next,
    Prev,
    NextGroup,
    PrevGroup,
    ToggleInteractive,
    SearchForward,
    SearchBackward,
    Undo,
    Redisplay,
    Search,
    ClearSearch,
    Insert(char),
    Backspace,
}

/// Result of processing a menu action
#[derive(Clone, Debug)]
pub enum MenuResult {
    Continue,
    Accept(String),
    AcceptAndContinue(String),
    Cancel,
    Redisplay,
    None,
}

impl MenuState {
    /// Process a menu action and return the result
    pub fn process_action(&mut self, action: MenuAction) -> MenuResult {
        match action {
            MenuAction::Accept => {
                if let Some(comp) = self.selected() {
                    let insert = comp.insert_str();
                    self.stop();
                    return MenuResult::Accept(insert);
                }
                MenuResult::Cancel
            }
            MenuAction::AcceptAndHold | MenuAction::AcceptAndInferNext => {
                if let Some(idx) = self.selected_idx {
                    if let Some(item) = self.items.get(idx) {
                        let insert = item.completion.insert_str();
                        self.navigate(MenuMotion::Next);
                        return MenuResult::AcceptAndContinue(insert);
                    }
                }
                MenuResult::None
            }
            MenuAction::Cancel => {
                self.stop();
                MenuResult::Cancel
            }
            MenuAction::Up => {
                self.navigate(MenuMotion::Up);
                MenuResult::Continue
            }
            MenuAction::Down => {
                self.navigate(MenuMotion::Down);
                MenuResult::Continue
            }
            MenuAction::Left => {
                self.navigate(MenuMotion::Left);
                MenuResult::Continue
            }
            MenuAction::Right => {
                self.navigate(MenuMotion::Right);
                MenuResult::Continue
            }
            MenuAction::PageUp => {
                self.navigate(MenuMotion::PageUp);
                MenuResult::Continue
            }
            MenuAction::PageDown => {
                self.navigate(MenuMotion::PageDown);
                MenuResult::Continue
            }
            MenuAction::Beginning => {
                self.navigate(MenuMotion::First);
                MenuResult::Continue
            }
            MenuAction::End => {
                self.navigate(MenuMotion::Last);
                MenuResult::Continue
            }
            MenuAction::BeginningOfLine | MenuAction::EndOfLine => MenuResult::Continue,
            MenuAction::Next => {
                self.navigate(MenuMotion::Next);
                MenuResult::Continue
            }
            MenuAction::Prev => {
                self.navigate(MenuMotion::Prev);
                MenuResult::Continue
            }
            MenuAction::NextGroup | MenuAction::PrevGroup => MenuResult::Continue,
            MenuAction::ToggleInteractive => MenuResult::Continue,
            MenuAction::SearchForward | MenuAction::SearchBackward | MenuAction::Search => {
                self.search_active = true;
                MenuResult::Continue
            }
            MenuAction::ClearSearch => {
                self.search_clear();
                MenuResult::Continue
            }
            MenuAction::Undo => MenuResult::None,
            MenuAction::Redisplay => MenuResult::Redisplay,
            MenuAction::Insert(c) => {
                if self.search_active {
                    self.search_input(c);
                }
                MenuResult::Continue
            }
            MenuAction::Backspace => {
                if self.search_active {
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
        for i in 0..20 {
            let mut comp = Completion::new(format!("item{:02}", i));
            comp.desc = Some(format!("description {}", i));
            group.matches.push(comp);
        }

        menu.set_completions(&[group]);

        assert_eq!(menu.count(), 20);
        assert!(!menu.is_active());

        menu.start();
        assert!(menu.is_active());
        assert_eq!(menu.selected_index(), Some(0));

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
pub fn default_menuselect_bindings() -> Vec<(&'static str, MenuAction)> {
    vec![
        ("accept-line", MenuAction::Accept),
        ("send-break", MenuAction::Cancel),
        ("accept-and-hold", MenuAction::AcceptAndHold),
        ("accept-and-menu-complete", MenuAction::AcceptAndHold),
        (
            "accept-and-infer-next-history",
            MenuAction::AcceptAndInferNext,
        ),
        ("down-history", MenuAction::Down),
        ("down-line-or-history", MenuAction::Down),
        ("up-history", MenuAction::Up),
        ("up-line-or-history", MenuAction::Up),
        ("forward-char", MenuAction::Right),
        ("vi-forward-char", MenuAction::Right),
        ("backward-char", MenuAction::Left),
        ("vi-backward-char", MenuAction::Left),
        ("forward-word", MenuAction::PageDown),
        ("vi-forward-word", MenuAction::PageDown),
        ("backward-word", MenuAction::PageUp),
        ("vi-backward-word", MenuAction::PageUp),
        ("vi-forward-blank-word", MenuAction::NextGroup),
        ("vi-backward-blank-word", MenuAction::PrevGroup),
        ("beginning-of-history", MenuAction::Beginning),
        ("end-of-history", MenuAction::End),
        ("vi-beginning-of-line", MenuAction::BeginningOfLine),
        ("vi-end-of-line", MenuAction::EndOfLine),
        ("complete-word", MenuAction::Next),
        ("menu-complete", MenuAction::Next),
        ("reverse-menu-complete", MenuAction::Prev),
        ("vi-insert", MenuAction::ToggleInteractive),
        (
            "history-incremental-search-forward",
            MenuAction::SearchForward,
        ),
        (
            "history-incremental-search-backward",
            MenuAction::SearchBackward,
        ),
        ("undo", MenuAction::Undo),
        ("redisplay", MenuAction::Redisplay),
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
    pub fn is_interactive(&self) -> bool {
        false
    }
    pub fn is_search_active(&self) -> bool {
        self.search_active
    }
    pub fn search_string(&self) -> &str {
        &self.search
    }
}
