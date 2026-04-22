//! ZPWR zstyle color configuration
//!
//! Complete color mappings from ~/.zpwr/autoload/common/zpwrBindZstyle
//! Format: tag -> (prefix_color, completion_color)
//! prefix_color is typically "1;30" (bold dark)
//! completion_color is the group-specific background color

use std::collections::HashMap;

/// Default prefix color for all completions (bold dark gray)
pub const DEFAULT_PREFIX_COLOR: &str = "1;30";

/// Menu selection color (ma=)
pub const MENU_SELECTION_COLOR: &str = "37;1;4;44"; // white bold underline on blue

/// Header colors from ZPWR_DESC_* env vars
#[derive(Clone, Debug)]
pub struct HeaderColors {
    pub pre: String,        // -<<
    pub post: String,       // >>-
    pub pre_color: String,  // 1;31 (bold red)
    pub text_color: String, // 34 (blue)
    pub post_color: String, // 1;31 (bold red)
}

impl Default for HeaderColors {
    fn default() -> Self {
        Self {
            pre: "-<<".to_string(),
            post: ">>-".to_string(),
            pre_color: "1;31".to_string(),
            text_color: "34".to_string(),
            post_color: "1;31".to_string(),
        }
    }
}

impl HeaderColors {
    pub fn from_env() -> Self {
        Self {
            pre: std::env::var("ZPWR_DESC_PRE").unwrap_or_else(|_| "-<<".to_string()),
            post: std::env::var("ZPWR_DESC_POST").unwrap_or_else(|_| ">>-".to_string()),
            pre_color: std::env::var("ZPWR_DESC_PRE_COLOR").unwrap_or_else(|_| "1;31".to_string()),
            text_color: std::env::var("ZPWR_DESC_TEXT_COLOR").unwrap_or_else(|_| "34".to_string()),
            post_color: std::env::var("ZPWR_DESC_POST_COLOR")
                .unwrap_or_else(|_| "1;31".to_string()),
        }
    }

    pub fn format(&self, text: &str) -> String {
        format!(
            "\x1b[{}m{}\x1b[0m\x1b[{}m{}\x1b[0m\x1b[{}m{}\x1b[0m",
            self.pre_color, self.pre, self.text_color, text, self.post_color, self.post
        )
    }
}

/// All ZPWR zstyle list-colors mappings
/// Returns HashMap<tag, completion_color>
pub fn zpwr_list_colors() -> HashMap<String, String> {
    let mut m = HashMap::new();

    // Core completion types
    m.insert("builtins".into(), "1;37;4;43".into()); // bold white underline on yellow
    m.insert("builtin command".into(), "1;37;4;43".into());
    m.insert("executables".into(), "1;37;44".into()); // bold white on blue
    m.insert("external command".into(), "1;37;44".into());
    m.insert("parameters".into(), "1;32;45".into()); // bold green on magenta
    m.insert("parameter".into(), "1;32;45".into());
    m.insert("abs-directories".into(), "1;32;45".into());
    m.insert("reserved-words".into(), "1;4;37;45".into()); // bold underline white on magenta
    m.insert("functions".into(), "1;37;41".into()); // bold white on red
    m.insert("shell function".into(), "1;37;41".into());

    // Aliases
    m.insert("aliases".into(), "34;42;4".into()); // blue on green underline
    m.insert("alias".into(), "34;42;4".into());
    m.insert("suffix-aliases".into(), "1;34;41;4".into()); // bold blue on red underline
    m.insert("global-aliases".into(), "1;34;43;4".into()); // bold blue on yellow underline

    // Users and hosts
    m.insert("users".into(), "1;37;42".into()); // bold white on green
    m.insert("hosts".into(), "1;37;43".into()); // bold white on yellow

    // Corrections
    m.insert("corrections".into(), "1;37;4;43".into());
    m.insert("original".into(), "34;42;4".into());

    // Git completions
    m.insert("commits".into(), "1;33;44".into()); // bold yellow on blue
    m.insert("heads".into(), "34;42;4".into());
    m.insert("commit-tags".into(), "1;34;41;4".into());
    m.insert("cached-files".into(), "1;34;41;4".into());
    m.insert("files".into(), "1;34;41;4".into());
    m.insert("blobs".into(), "1;34;41;4".into());
    m.insert("blob-objects".into(), "1;34;41;4".into());
    m.insert("trees".into(), "1;34;41;4".into());
    m.insert("tags".into(), "1;34;41;4".into());
    m.insert("heads-local".into(), "1;34;43;4".into());
    m.insert("heads-remote".into(), "1;37;46".into()); // bold white on cyan
    m.insert("modified-files".into(), "1;37;42".into());
    m.insert("revisions".into(), "1;37;42".into());
    m.insert("recent-branches".into(), "1;37;44".into());
    m.insert("remote-branch-names-noprefix".into(), "1;33;46".into());
    m.insert("blobs-and-trees-in-treeish".into(), "1;34;43".into());
    m.insert("commit-objects".into(), "1;37;43".into());
    m.insert("prefixes".into(), "1;37;43".into());

    // Directories
    m.insert("directory".into(), "1;32;45".into());
    m.insert("local-directories".into(), "1;32;45".into());

    // Manual sections
    m.insert("manuals.1".into(), "1;36;44".into());
    m.insert("manuals.2".into(), "1;37;42".into());
    m.insert("manuals.3".into(), "1;37;43".into());
    m.insert("manuals.4".into(), "37;46".into());
    m.insert("manuals.5".into(), "1;34;43;4".into());
    m.insert("manuals.6".into(), "1;37;41".into());
    m.insert("manuals.7".into(), "34;42;4".into());
    m.insert("manuals.8".into(), "1;34;41;4".into());
    m.insert("manuals.9".into(), "1;36;44".into());
    m.insert("manuals.n".into(), "1;4;37;45".into());
    m.insert("manuals.0p".into(), "37;46".into());
    m.insert("manuals.1p".into(), "37;46".into());
    m.insert("manuals.3p".into(), "37;46".into());

    // Remote packages
    m.insert("cpan-module".into(), "37;46".into());
    m.insert("remote-pip".into(), "37;46".into());
    m.insert("remote-gem".into(), "37;46".into());
    m.insert("remote-crate".into(), "1;36;44".into());

    // Processes
    m.insert("processes".into(), "1;36;44".into());
    m.insert("processes-names".into(), "1;37;43".into());

    // ZPWR verbs
    m.insert("zpwr-vim".into(), "1;36;44".into());
    m.insert("zpwr-emacs".into(), "1;37;45".into());
    m.insert("zpwr-regen".into(), "1;32;45".into());
    m.insert("zpwr-clean".into(), "1;4;37;45".into());
    m.insert("zpwr-send".into(), "33;45".into());
    m.insert("zpwr-misc".into(), "37;46".into());
    m.insert("zpwr-travis".into(), "1;34;41".into());
    m.insert("zpwr-learn".into(), "1;32;44".into());
    m.insert("zpwr-search".into(), "1;34;43".into());
    m.insert("zpwr-update".into(), "1;37;46".into());
    m.insert("zpwr-cd".into(), "1;37;42".into());
    m.insert("zpwr-forgit".into(), "34;42".into());
    m.insert("zpwr-git".into(), "1;37;41".into());
    m.insert("zpwr-github".into(), "1;4;37;45".into());
    m.insert("zpwr-gitrepos".into(), "1;4;36;44".into());
    m.insert("zpwr-clipboard".into(), "1;36;44".into());
    m.insert("zpwr-log".into(), "1;32;45".into());
    m.insert("zpwr-diag".into(), "1;33;44".into());
    m.insert("zpwr-monitor".into(), "1;35;42".into());

    // Options
    m.insert("options".into(), "1;37;44".into());

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_colors_default() {
        let hc = HeaderColors::default();
        assert_eq!(hc.pre, "-<<");
        assert_eq!(hc.post, ">>-");
        assert_eq!(hc.pre_color, "1;31");
        assert_eq!(hc.text_color, "34");
        assert_eq!(hc.post_color, "1;31");
    }

    #[test]
    fn test_header_format() {
        let hc = HeaderColors::default();
        let formatted = hc.format("test");
        assert!(formatted.contains("-<<"));
        assert!(formatted.contains("test"));
        assert!(formatted.contains(">>-"));
        assert!(formatted.contains("\x1b[1;31m")); // pre color
        assert!(formatted.contains("\x1b[34m")); // text color
    }

    #[test]
    fn test_zpwr_list_colors_has_all_core_types() {
        let colors = zpwr_list_colors();

        // Core types must be present
        assert!(colors.contains_key("builtins"));
        assert!(colors.contains_key("executables"));
        assert!(colors.contains_key("parameters"));
        assert!(colors.contains_key("functions"));
        assert!(colors.contains_key("aliases"));
        assert!(colors.contains_key("alias"));

        // Friendly names
        assert!(colors.contains_key("builtin command"));
        assert!(colors.contains_key("external command"));
        assert!(colors.contains_key("shell function"));
        assert!(colors.contains_key("parameter"));
    }

    #[test]
    fn test_zpwr_colors_format() {
        let colors = zpwr_list_colors();

        // All colors should be valid ANSI codes (semicolon-separated numbers)
        for (tag, color) in &colors {
            for part in color.split(';') {
                assert!(
                    part.parse::<u32>().is_ok(),
                    "Invalid color code '{}' for tag '{}'",
                    color,
                    tag
                );
            }
        }
    }

    #[test]
    fn test_builtins_color() {
        let colors = zpwr_list_colors();
        assert_eq!(colors.get("builtins"), Some(&"1;37;4;43".to_string()));
    }

    #[test]
    fn test_executables_color() {
        let colors = zpwr_list_colors();
        assert_eq!(colors.get("executables"), Some(&"1;37;44".to_string()));
    }

    #[test]
    fn test_functions_color() {
        let colors = zpwr_list_colors();
        assert_eq!(colors.get("functions"), Some(&"1;37;41".to_string()));
    }

    #[test]
    fn test_aliases_color() {
        let colors = zpwr_list_colors();
        assert_eq!(colors.get("aliases"), Some(&"34;42;4".to_string()));
    }

    #[test]
    fn test_parameters_color() {
        let colors = zpwr_list_colors();
        assert_eq!(colors.get("parameters"), Some(&"1;32;45".to_string()));
    }

    #[test]
    fn test_git_colors() {
        let colors = zpwr_list_colors();
        assert!(colors.contains_key("commits"));
        assert!(colors.contains_key("heads"));
        assert!(colors.contains_key("heads-local"));
        assert!(colors.contains_key("heads-remote"));
        assert!(colors.contains_key("recent-branches"));
    }

    #[test]
    fn test_zpwr_verbs() {
        let colors = zpwr_list_colors();
        assert!(colors.contains_key("zpwr-vim"));
        assert!(colors.contains_key("zpwr-git"));
        assert!(colors.contains_key("zpwr-cd"));
    }

    #[test]
    fn test_manual_sections() {
        let colors = zpwr_list_colors();
        for i in 1..=9 {
            let key = format!("manuals.{}", i);
            assert!(colors.contains_key(&key), "Missing {}", key);
        }
        assert!(colors.contains_key("manuals.n"));
    }
}
