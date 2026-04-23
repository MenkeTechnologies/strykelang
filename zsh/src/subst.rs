//! Substitution handling for zshrs
//!
//! Port from zsh/Src/subst.c
//!
//! Provides parameter expansion, command substitution, arithmetic expansion,
//! brace expansion, tilde expansion, and filename generation.

use std::collections::HashMap;
use std::env;
use std::process::{Command, Stdio};

/// Prefork flags
pub mod prefork {
    pub const SINGLE: u32 = 1;       // Single word expected
    pub const SPLIT: u32 = 2;        // Force word splitting
    pub const SHWORDSPLIT: u32 = 4;  // sh-style word splitting
    pub const NOSHWORDSPLIT: u32 = 8; // Disable word splitting
    pub const ASSIGN: u32 = 16;      // Assignment context
    pub const TYPESET: u32 = 32;     // Typeset context
    pub const SUBEXP: u32 = 64;      // Subexpression
    pub const KEY_VALUE: u32 = 128;  // Key-value pair found
}

/// Perform all substitutions on a word
pub fn subst_string(s: &str, params: &HashMap<String, String>, opts: &SubstOptions) -> Result<String, String> {
    let mut result = s.to_string();

    // Tilde expansion
    result = tilde_expand(&result, opts)?;

    // Parameter expansion
    result = param_expand(&result, params, opts)?;

    // Command substitution
    result = command_subst(&result, opts)?;

    // Arithmetic expansion
    result = arith_expand(&result, params, opts)?;

    Ok(result)
}

/// Substitution options
#[derive(Clone, Debug, Default)]
pub struct SubstOptions {
    pub noglob: bool,
    pub noexec: bool,
    pub nounset: bool,
    pub word_split: bool,
    pub ignore_braces: bool,
}

/// Tilde expansion
pub fn tilde_expand(s: &str, _opts: &SubstOptions) -> Result<String, String> {
    if !s.starts_with('~') {
        return Ok(s.to_string());
    }

    let rest = &s[1..];
    
    // Find end of username
    let (user, suffix) = match rest.find('/') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, ""),
    };

    let expanded = if user.is_empty() {
        // ~ alone means $HOME
        env::var("HOME").unwrap_or_else(|_| "/".to_string())
    } else if user.starts_with('+') {
        // ~+ means $PWD
        env::var("PWD").unwrap_or_else(|_| ".".to_string())
    } else if user.starts_with('-') {
        // ~- means $OLDPWD
        env::var("OLDPWD").unwrap_or_else(|_| ".".to_string())
    } else {
        // ~user means user's home directory
        #[cfg(unix)]
        {
            get_user_home(user).unwrap_or_else(|| format!("~{}", user))
        }
        #[cfg(not(unix))]
        {
            format!("~{}", user)
        }
    };

    Ok(format!("{}{}", expanded, suffix))
}

#[cfg(unix)]
fn get_user_home(user: &str) -> Option<String> {
    use std::ffi::CString;
    unsafe {
        let c_user = CString::new(user).ok()?;
        let pw = libc::getpwnam(c_user.as_ptr());
        if pw.is_null() {
            return None;
        }
        let dir = std::ffi::CStr::from_ptr((*pw).pw_dir);
        dir.to_str().ok().map(|s| s.to_string())
    }
}

/// Parameter expansion
pub fn param_expand(s: &str, params: &HashMap<String, String>, opts: &SubstOptions) -> Result<String, String> {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            match chars.peek() {
                Some(&'{') => {
                    // ${...} form
                    chars.next();
                    let expanded = parse_brace_param(&mut chars, params, opts)?;
                    result.push_str(&expanded);
                }
                Some(&'(') => {
                    // $(...) command substitution or $((...)) arithmetic
                    chars.next();
                    if chars.peek() == Some(&'(') {
                        // $((...)) arithmetic
                        chars.next();
                        let expr = collect_until(&mut chars, ')');
                        if chars.next() != Some(')') {
                            return Err("Missing )) in arithmetic expansion".to_string());
                        }
                        let value = eval_arith(&expr, params)?;
                        result.push_str(&value.to_string());
                    } else {
                        // $(...) command substitution
                        let cmd = collect_balanced(&mut chars, '(', ')');
                        if !opts.noexec {
                            let output = run_command(&cmd)?;
                            result.push_str(output.trim_end_matches('\n'));
                        }
                    }
                }
                Some(&c) if c.is_ascii_alphabetic() || c == '_' => {
                    // Simple $var - consume first char we peeked
                    chars.next();
                    let name = collect_varname(&mut chars);
                    let full_name = format!("{}{}", c, name);
                    
                    if let Some(value) = params.get(&full_name) {
                        result.push_str(value);
                    } else if let Ok(value) = env::var(&full_name) {
                        result.push_str(&value);
                    } else if opts.nounset {
                        return Err(format!("{}: parameter not set", full_name));
                    }
                }
                Some(&c) if c.is_ascii_digit() => {
                    // Positional parameter
                    let mut num = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() {
                            num.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    // Positional params would be looked up here
                }
                Some(&'?') => {
                    chars.next();
                    result.push_str("0"); // Last exit status
                }
                Some(&'$') => {
                    chars.next();
                    result.push_str(&std::process::id().to_string());
                }
                Some(&'#') => {
                    chars.next();
                    result.push_str("0"); // Number of positional params
                }
                Some(&'*') | Some(&'@') => {
                    chars.next();
                    // All positional params
                }
                _ => result.push('$'),
            }
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

fn parse_brace_param(chars: &mut std::iter::Peekable<std::str::Chars>, 
                     params: &HashMap<String, String>,
                     opts: &SubstOptions) -> Result<String, String> {
    let mut name = String::new();
    let mut operator = None;
    let mut operand = String::new();
    
    // Check for special prefix operators
    let prefix = match chars.peek() {
        Some(&'#') => {
            chars.next();
            if chars.peek() == Some(&'}') {
                // ${#} - number of params
                chars.next();
                return Ok("0".to_string());
            }
            Some('#') // Length operator
        }
        Some(&'!') => {
            chars.next();
            Some('!') // Indirect expansion
        }
        _ => None,
    };

    // Collect variable name
    while let Some(&c) = chars.peek() {
        if c.is_ascii_alphanumeric() || c == '_' {
            name.push(chars.next().unwrap());
        } else {
            break;
        }
    }

    // Check for operators
    match chars.peek() {
        Some(&':') => {
            chars.next();
            match chars.peek() {
                Some(&'-') => { chars.next(); operator = Some(":-"); }
                Some(&'=') => { chars.next(); operator = Some(":="); }
                Some(&'+') => { chars.next(); operator = Some(":+"); }
                Some(&'?') => { chars.next(); operator = Some(":?"); }
                _ => operator = Some(":"),
            }
        }
        Some(&'-') => { chars.next(); operator = Some("-"); }
        Some(&'=') => { chars.next(); operator = Some("="); }
        Some(&'+') => { chars.next(); operator = Some("+"); }
        Some(&'?') => { chars.next(); operator = Some("?"); }
        Some(&'#') => { chars.next(); operator = Some("#"); }
        Some(&'%') => { chars.next(); operator = Some("%"); }
        Some(&'/') => { chars.next(); operator = Some("/"); }
        Some(&'^') => { chars.next(); operator = Some("^"); }
        Some(&',') => { chars.next(); operator = Some(","); }
        _ => {}
    }

    // Collect operand until closing brace
    let mut depth = 1;
    while depth > 0 {
        match chars.next() {
            Some('{') => depth += 1,
            Some('}') => depth -= 1,
            Some(c) if depth > 0 => operand.push(c),
            None => return Err("Missing } in parameter expansion".to_string()),
            _ => {}
        }
    }

    // Get the value
    let value = params.get(&name)
        .cloned()
        .or_else(|| env::var(&name).ok());

    // Handle prefix operators
    if let Some('#') = prefix {
        // ${#var} - length
        return Ok(value.map(|v| v.len()).unwrap_or(0).to_string());
    }

    // Apply operator
    match operator {
        Some(":-") | Some("-") => {
            if value.as_ref().map(|v| v.is_empty()).unwrap_or(true) {
                Ok(operand)
            } else {
                Ok(value.unwrap_or_default())
            }
        }
        Some(":+") | Some("+") => {
            if value.as_ref().map(|v| !v.is_empty()).unwrap_or(false) {
                Ok(operand)
            } else {
                Ok(String::new())
            }
        }
        Some(":?") | Some("?") => {
            if value.as_ref().map(|v| v.is_empty()).unwrap_or(true) {
                Err(if operand.is_empty() {
                    format!("{}: parameter null or not set", name)
                } else {
                    operand
                })
            } else {
                Ok(value.unwrap_or_default())
            }
        }
        Some("#") => {
            // Remove shortest prefix
            if let Some(v) = value {
                Ok(remove_prefix(&v, &operand, false))
            } else {
                Ok(String::new())
            }
        }
        Some("##") => {
            // Remove longest prefix
            if let Some(v) = value {
                Ok(remove_prefix(&v, &operand, true))
            } else {
                Ok(String::new())
            }
        }
        Some("%") => {
            // Remove shortest suffix
            if let Some(v) = value {
                Ok(remove_suffix(&v, &operand, false))
            } else {
                Ok(String::new())
            }
        }
        Some("%%") => {
            // Remove longest suffix
            if let Some(v) = value {
                Ok(remove_suffix(&v, &operand, true))
            } else {
                Ok(String::new())
            }
        }
        Some("^") => {
            // Uppercase first char
            if let Some(v) = value {
                let mut c = v.chars();
                match c.next() {
                    Some(first) => Ok(first.to_uppercase().collect::<String>() + c.as_str()),
                    None => Ok(String::new()),
                }
            } else {
                Ok(String::new())
            }
        }
        Some("^^") => {
            // Uppercase all
            Ok(value.map(|v| v.to_uppercase()).unwrap_or_default())
        }
        Some(",") => {
            // Lowercase first char
            if let Some(v) = value {
                let mut c = v.chars();
                match c.next() {
                    Some(first) => Ok(first.to_lowercase().collect::<String>() + c.as_str()),
                    None => Ok(String::new()),
                }
            } else {
                Ok(String::new())
            }
        }
        Some(",,") => {
            // Lowercase all
            Ok(value.map(|v| v.to_lowercase()).unwrap_or_default())
        }
        Some("/") => {
            // Substitution
            if let Some(v) = value {
                let parts: Vec<&str> = operand.splitn(2, '/').collect();
                if parts.len() == 2 {
                    Ok(v.replacen(parts[0], parts[1], 1))
                } else {
                    Ok(v.replacen(parts[0], "", 1))
                }
            } else {
                Ok(String::new())
            }
        }
        Some("//") => {
            // Global substitution
            if let Some(v) = value {
                let parts: Vec<&str> = operand.splitn(2, '/').collect();
                if parts.len() == 2 {
                    Ok(v.replace(parts[0], parts[1]))
                } else {
                    Ok(v.replace(parts[0], ""))
                }
            } else {
                Ok(String::new())
            }
        }
        _ => Ok(value.unwrap_or_default()),
    }
}

fn remove_prefix(s: &str, pattern: &str, greedy: bool) -> String {
    // Simple glob-less version
    if greedy {
        for i in (0..=s.len()).rev() {
            if s[..i].ends_with(pattern) || (pattern == "*" && i > 0) {
                return s[i..].to_string();
            }
        }
    } else {
        for i in 0..=s.len() {
            if s[..i].ends_with(pattern) || (pattern == "*" && i > 0) {
                return s[i..].to_string();
            }
        }
    }
    s.to_string()
}

fn remove_suffix(s: &str, pattern: &str, greedy: bool) -> String {
    // Simple glob-less version
    if greedy {
        for i in 0..=s.len() {
            if s[i..].starts_with(pattern) || (pattern == "*" && i < s.len()) {
                return s[..i].to_string();
            }
        }
    } else {
        for i in (0..=s.len()).rev() {
            if s[i..].starts_with(pattern) || (pattern == "*" && i < s.len()) {
                return s[..i].to_string();
            }
        }
    }
    s.to_string()
}

fn collect_varname(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut name = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_alphanumeric() || c == '_' {
            name.push(chars.next().unwrap());
        } else {
            break;
        }
    }
    name
}

fn collect_until(chars: &mut std::iter::Peekable<std::str::Chars>, end: char) -> String {
    let mut result = String::new();
    while let Some(&c) = chars.peek() {
        if c == end {
            break;
        }
        result.push(chars.next().unwrap());
    }
    result
}

fn collect_balanced(chars: &mut std::iter::Peekable<std::str::Chars>, 
                   open: char, close: char) -> String {
    let mut result = String::new();
    let mut depth = 1;
    
    while depth > 0 {
        match chars.next() {
            Some(c) if c == open => {
                depth += 1;
                result.push(c);
            }
            Some(c) if c == close => {
                depth -= 1;
                if depth > 0 {
                    result.push(c);
                }
            }
            Some(c) => result.push(c),
            None => break,
        }
    }
    
    result
}

/// Command substitution
pub fn command_subst(s: &str, opts: &SubstOptions) -> Result<String, String> {
    if opts.noexec {
        return Ok(s.to_string());
    }

    let mut result = String::new();
    let mut chars = s.chars().peekable();
    
    while let Some(c) = chars.next() {
        if c == '`' {
            // Backtick form
            let cmd = collect_until(&mut chars, '`');
            chars.next(); // consume closing backtick
            let output = run_command(&cmd)?;
            result.push_str(output.trim_end_matches('\n'));
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

fn run_command(cmd: &str) -> Result<String, String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| e.to_string())?;

    String::from_utf8(output.stdout)
        .map_err(|e| e.to_string())
}

/// Arithmetic expansion
pub fn arith_expand(s: &str, params: &HashMap<String, String>, opts: &SubstOptions) -> Result<String, String> {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'[') {
            // $[...] form
            chars.next();
            let expr = collect_until(&mut chars, ']');
            chars.next(); // consume ]
            
            if !opts.noexec {
                let value = eval_arith(&expr, params)?;
                result.push_str(&value.to_string());
            }
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

fn eval_arith(expr: &str, _params: &HashMap<String, String>) -> Result<i64, String> {
    // Simple arithmetic evaluation
    // This would use the math module in practice
    let expr = expr.trim();
    
    // Handle simple integers
    if let Ok(n) = expr.parse::<i64>() {
        return Ok(n);
    }

    // Try simple expression
    if let Some(pos) = expr.find('+') {
        let left = expr[..pos].trim().parse::<i64>().map_err(|e| e.to_string())?;
        let right = expr[pos+1..].trim().parse::<i64>().map_err(|e| e.to_string())?;
        return Ok(left + right);
    }
    if let Some(pos) = expr.rfind('-') {
        if pos > 0 {
            let left = expr[..pos].trim().parse::<i64>().map_err(|e| e.to_string())?;
            let right = expr[pos+1..].trim().parse::<i64>().map_err(|e| e.to_string())?;
            return Ok(left - right);
        }
    }
    if let Some(pos) = expr.find('*') {
        let left = expr[..pos].trim().parse::<i64>().map_err(|e| e.to_string())?;
        let right = expr[pos+1..].trim().parse::<i64>().map_err(|e| e.to_string())?;
        return Ok(left * right);
    }
    if let Some(pos) = expr.find('/') {
        let left = expr[..pos].trim().parse::<i64>().map_err(|e| e.to_string())?;
        let right = expr[pos+1..].trim().parse::<i64>().map_err(|e| e.to_string())?;
        if right == 0 {
            return Err("division by zero".to_string());
        }
        return Ok(left / right);
    }

    Err(format!("Invalid arithmetic expression: {}", expr))
}

/// Brace expansion
pub fn brace_expand(s: &str) -> Vec<String> {
    if !s.contains('{') {
        return vec![s.to_string()];
    }

    let mut results = vec![String::new()];
    let mut chars = s.chars().peekable();
    
    while let Some(c) = chars.next() {
        if c == '{' {
            let content = collect_balanced(&mut chars, '{', '}');
            let alternatives: Vec<&str> = content.split(',').collect();
            
            if alternatives.len() > 1 {
                results = results.iter()
                    .flat_map(|prefix| {
                        alternatives.iter()
                            .map(|alt| format!("{}{}", prefix, alt))
                            .collect::<Vec<_>>()
                    })
                    .collect();
            } else if let Some((start, end)) = parse_range(&content) {
                results = results.iter()
                    .flat_map(|prefix| {
                        (start..=end)
                            .map(|n| format!("{}{}", prefix, n))
                            .collect::<Vec<_>>()
                    })
                    .collect();
            } else {
                for r in &mut results {
                    r.push('{');
                    r.push_str(&content);
                    r.push('}');
                }
            }
        } else {
            for r in &mut results {
                r.push(c);
            }
        }
    }

    results
}

fn parse_range(s: &str) -> Option<(i32, i32)> {
    let parts: Vec<&str> = s.splitn(2, "..").collect();
    if parts.len() != 2 {
        return None;
    }
    let start = parts[0].parse().ok()?;
    let end = parts[1].parse().ok()?;
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tilde_expand() {
        let opts = SubstOptions::default();
        let result = tilde_expand("~", &opts).unwrap();
        assert!(!result.starts_with('~'));
    }

    #[test]
    fn test_param_expand_simple() {
        let mut params = HashMap::new();
        params.insert("FOO".to_string(), "bar".to_string());
        
        let opts = SubstOptions::default();
        let result = param_expand("$FOO", &params, &opts).unwrap();
        assert_eq!(result, "bar");
    }

    #[test]
    fn test_param_expand_default() {
        let params = HashMap::new();
        let opts = SubstOptions::default();
        
        let result = param_expand("${UNDEFINED:-default}", &params, &opts).unwrap();
        assert_eq!(result, "default");
    }

    #[test]
    fn test_brace_expand_alternatives() {
        let results = brace_expand("file.{txt,md,rs}");
        assert_eq!(results.len(), 3);
        assert!(results.contains(&"file.txt".to_string()));
        assert!(results.contains(&"file.md".to_string()));
        assert!(results.contains(&"file.rs".to_string()));
    }

    #[test]
    fn test_brace_expand_range() {
        let results = brace_expand("file{1..3}");
        assert_eq!(results.len(), 3);
        assert!(results.contains(&"file1".to_string()));
        assert!(results.contains(&"file2".to_string()));
        assert!(results.contains(&"file3".to_string()));
    }
}
