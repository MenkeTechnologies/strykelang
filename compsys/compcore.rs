//! Completion core - main entry point and match processing
//!
//! Ported from zsh Src/Zle/compcore.c with patterns from fish complete.rs

use crate::completion::{Completion, CompletionFlags, CompletionGroup};
use crate::state::CompParams;
use crate::zstyle::ZStyleStore;

/// Completion request options (inspired by fish)
#[derive(Clone, Copy, Debug, Default)]
pub struct CompletionRequestOptions {
    /// This is for autosuggestion (single best match)
    pub autosuggestion: bool,
    /// Generate descriptions for completions
    pub descriptions: bool,
    /// Allow fuzzy matching
    pub fuzzy_match: bool,
}

impl CompletionRequestOptions {
    pub fn autosuggest() -> Self {
        Self {
            autosuggestion: true,
            descriptions: false,
            fuzzy_match: false,
        }
    }

    pub fn normal() -> Self {
        Self {
            autosuggestion: false,
            descriptions: true,
            fuzzy_match: true,
        }
    }
}

/// Completion mode flags (from fish, maps to zsh options)
#[derive(Clone, Copy, Debug, Default)]
pub struct CompletionMode {
    /// Skip file completions
    pub no_files: bool,
    /// Force file completions even with other completions
    pub force_files: bool,
    /// Require a parameter after completion
    pub requires_param: bool,
}

/// Ambiguous match info (zsh ainfo/fainfo)
#[derive(Clone, Debug, Default)]
pub struct AmbiguousInfo {
    /// Unambiguous prefix
    pub prefix: String,
    /// Unambiguous suffix  
    pub suffix: String,
    /// Length of prefix
    pub prefix_len: usize,
    /// Length of suffix
    pub suffix_len: usize,
    /// Whether there's an exact match
    pub exact: bool,
    /// The exact match string
    pub exact_string: String,
}

/// Menu completion info
#[derive(Clone, Debug, Default)]
pub struct MenuInfo {
    /// Current match index (if in menu mode)
    pub current: Option<usize>,
    /// Group of current match
    pub group: Option<String>,
    /// Whether user was asked about listing
    pub asked: bool,
}

/// Global completion state during a completion operation
#[derive(Debug)]
pub struct CompletionState {
    /// The matches organized by group
    pub groups: Vec<CompletionGroup>,
    /// Total number of matches
    pub nmatches: usize,
    /// Number of matches to display (excludes hidden)
    pub smatches: usize,
    /// Whether there are different matches (not all identical)
    pub diff_matches: bool,
    /// Number of messages/explanations
    pub nmessages: usize,
    /// Ambiguous info for normal matches
    pub ainfo: AmbiguousInfo,
    /// Ambiguous info for fignore matches
    pub fainfo: AmbiguousInfo,
    /// Menu completion state
    pub minfo: MenuInfo,
    /// Has pattern matching
    pub has_pattern: bool,
    /// Has exact match
    pub has_exact: bool,
    /// Current completion mode
    pub mode: CompletionMode,
    /// Style store for zstyle lookups
    pub styles: ZStyleStore,
    /// Completion parameters
    pub params: CompParams,
    /// Ignored match count
    pub ignored: usize,
}

impl CompletionState {
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            nmatches: 0,
            smatches: 0,
            diff_matches: false,
            nmessages: 0,
            ainfo: AmbiguousInfo::default(),
            fainfo: AmbiguousInfo::default(),
            minfo: MenuInfo::default(),
            has_pattern: false,
            has_exact: false,
            mode: CompletionMode::default(),
            styles: ZStyleStore::new(),
            params: CompParams::new(),
            ignored: 0,
        }
    }

    /// Initialize completion state from command line
    pub fn from_line(line: &str, cursor: usize) -> Self {
        let mut state = Self::new();
        state.params = CompParams::from_line(line, cursor);
        state
    }

    /// Get the current completion context string for zstyle
    pub fn context_string(&self) -> String {
        format!(
            ":completion:{}:{}:",
            self.params.compstate.context.as_str(),
            "" // completer name would go here
        )
    }

    /// Begin a new completion group
    pub fn begin_group(&mut self, name: &str, sorted: bool) {
        // Check if group already exists
        if let Some(group) = self.groups.iter_mut().find(|g| g.name == name) {
            group.sorted = sorted;
            return;
        }
        let group = if sorted {
            CompletionGroup::new(name)
        } else {
            CompletionGroup::new_unsorted(name)
        };
        self.groups.push(group);
    }

    /// End current group (finalize)
    pub fn end_group(&mut self) {
        // Sort if needed
        if let Some(group) = self.groups.last_mut() {
            if group.sorted {
                group.matches.sort_by(|a, b| a.str_.cmp(&b.str_));
            }
        }
    }

    /// Add a match to the current or specified group
    pub fn add_match(&mut self, comp: Completion, group_name: Option<&str>) {
        let group_name = group_name.unwrap_or("default");

        // Find or create group
        let group = if let Some(g) = self.groups.iter_mut().find(|g| g.name == group_name) {
            g
        } else {
            self.groups.push(CompletionGroup::new(group_name));
            self.groups.last_mut().unwrap()
        };

        // Track if matches differ
        if !group.matches.is_empty() && group.matches[0].str_ != comp.str_ {
            self.diff_matches = true;
        }

        // Add match
        if !comp.flags.contains(CompletionFlags::NOLIST) {
            self.smatches += 1;
        }
        self.nmatches += 1;
        group.add_match(comp);
    }

    /// Add an explanation/message to the current group
    pub fn add_explanation(&mut self, exp: String, group_name: Option<&str>) {
        let group_name = group_name.unwrap_or("default");
        if let Some(group) = self.groups.iter_mut().find(|g| g.name == group_name) {
            group.add_explanation(exp);
            self.nmessages += 1;
        }
    }

    /// Calculate the unambiguous prefix across all matches
    pub fn calculate_unambiguous(&mut self) {
        let all_matches: Vec<&Completion> =
            self.groups.iter().flat_map(|g| g.matches.iter()).collect();

        if all_matches.is_empty() {
            return;
        }

        if all_matches.len() == 1 {
            self.ainfo.prefix = all_matches[0].str_.clone();
            self.ainfo.prefix_len = self.ainfo.prefix.len();
            self.ainfo.exact = true;
            self.ainfo.exact_string = all_matches[0].str_.clone();
            return;
        }

        // Find common prefix
        let first = &all_matches[0].str_;
        let mut common_len = first.len();

        for m in &all_matches[1..] {
            let match_len = first
                .chars()
                .zip(m.str_.chars())
                .take_while(|(a, b)| a == b)
                .count();
            common_len = common_len.min(match_len);
        }

        self.ainfo.prefix = first.chars().take(common_len).collect();
        self.ainfo.prefix_len = common_len;

        // Check for exact match
        for m in &all_matches {
            if m.str_ == self.params.prefix {
                self.ainfo.exact = true;
                self.ainfo.exact_string = m.str_.clone();
                break;
            }
        }
    }

    /// Get all completions as a flat list
    pub fn all_completions(&self) -> Vec<&Completion> {
        self.groups.iter().flat_map(|g| g.matches.iter()).collect()
    }

    /// Update compstate based on current matches
    pub fn update_compstate(&mut self) {
        self.params.compstate.nmatches = self.nmatches as i32;
        self.params.compstate.ignored = self.ignored as i32;
        self.params.compstate.unambiguous = self.ainfo.prefix.clone();
        self.params.compstate.unambiguous_cursor = self.ainfo.prefix_len as i32;

        if self.ainfo.exact {
            self.params.compstate.exact_string = self.ainfo.exact_string.clone();
        }
    }
}

impl Default for CompletionState {
    fn default() -> Self {
        Self::new()
    }
}

/// Sort and prioritize completions (from fish, adapted for zsh groups)
pub fn sort_and_prioritize(groups: &mut [CompletionGroup], options: &CompletionRequestOptions) {
    for group in groups.iter_mut() {
        if group.matches.is_empty() {
            continue;
        }

        // Find best rank (if using fuzzy matching)
        if options.fuzzy_match {
            // For now, all matches have equal rank
            // TODO: implement fuzzy match ranking
        }

        // Deduplicate
        let mut seen = std::collections::HashSet::new();
        group.matches.retain(|c| seen.insert(c.str_.clone()));

        // Sort if the group is marked for sorting
        if group.sorted {
            group.matches.sort_by(|a, b| {
                // Files ending in ~ (autosave) sort last
                let a_tilde = a.str_.ends_with('~');
                let b_tilde = b.str_.ends_with('~');
                if a_tilde != b_tilde {
                    return a_tilde.cmp(&b_tilde);
                }
                a.str_.cmp(&b.str_)
            });
        }

        // Update lcount
        group.lcount = group
            .matches
            .iter()
            .filter(|m| !m.flags.contains(CompletionFlags::NOLIST))
            .count();
    }
}

/// Main completion entry point
///
/// This is called when completion is requested on a command line.
/// Returns the number of matches found.
pub fn do_completion(
    line: &str,
    cursor: usize,
    state: &mut CompletionState,
    completion_func: impl FnOnce(&mut CompletionState),
) -> usize {
    // Initialize state
    state.params = CompParams::from_line(line, cursor);
    state.nmatches = 0;
    state.smatches = 0;
    state.nmessages = 0;
    state.ignored = 0;
    state.groups.clear();

    // Call the completion function (this is where shell functions like _main_complete would run)
    completion_func(state);

    // Calculate unambiguous prefix
    state.calculate_unambiguous();

    // Sort and prioritize
    sort_and_prioritize(&mut state.groups, &CompletionRequestOptions::normal());

    // Update compstate
    state.update_compstate();

    state.nmatches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_state_basic() {
        let mut state = CompletionState::from_line("git ch", 6);

        assert_eq!(state.params.prefix, "ch");
        assert_eq!(state.params.current, 2);

        state.begin_group("commands", true);
        state.add_match(Completion::new("checkout"), Some("commands"));
        state.add_match(Completion::new("cherry-pick"), Some("commands"));

        assert_eq!(state.nmatches, 2);
        assert!(state.diff_matches);

        state.calculate_unambiguous();
        // "checkout" and "cherry-pick" share "che" as common prefix
        assert_eq!(state.ainfo.prefix, "che");
    }

    #[test]
    fn test_unambiguous_single_match() {
        let mut state = CompletionState::new();
        state.add_match(Completion::new("foobar"), None);
        state.calculate_unambiguous();

        assert_eq!(state.ainfo.prefix, "foobar");
        assert!(state.ainfo.exact);
    }

    #[test]
    fn test_unambiguous_multiple_matches() {
        let mut state = CompletionState::new();
        state.add_match(Completion::new("checkout"), None);
        state.add_match(Completion::new("cherry-pick"), None);
        state.add_match(Completion::new("clean"), None);
        state.calculate_unambiguous();

        assert_eq!(state.ainfo.prefix, "c");
        assert!(!state.ainfo.exact);
    }

    #[test]
    fn test_sort_and_prioritize() {
        let mut groups = vec![CompletionGroup::new("test")];
        groups[0].matches = vec![
            Completion::new("zebra"),
            Completion::new("alpha"),
            Completion::new("beta"),
            Completion::new("alpha"), // duplicate
        ];

        sort_and_prioritize(&mut groups, &CompletionRequestOptions::normal());

        assert_eq!(groups[0].matches.len(), 3); // deduplicated
        assert_eq!(groups[0].matches[0].str_, "alpha");
        assert_eq!(groups[0].matches[1].str_, "beta");
        assert_eq!(groups[0].matches[2].str_, "zebra");
    }
}
