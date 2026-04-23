//! Parameter management for zshrs
//!
//! Port from zsh/Src/params.c
//!
//! Provides shell parameters (variables), special parameters, arrays,
//! associative arrays, and parameter attributes.

use std::collections::HashMap;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

/// Parameter flags
pub mod flags {
    pub const SCALAR: u32 = 1 << 0;     // Scalar parameter
    pub const INTEGER: u32 = 1 << 1;    // Integer parameter
    pub const FLOAT: u32 = 1 << 2;      // Float parameter
    pub const ARRAY: u32 = 1 << 3;      // Array parameter
    pub const ASSOC: u32 = 1 << 4;      // Associative array
    pub const READONLY: u32 = 1 << 5;   // Read-only
    pub const SPECIAL: u32 = 1 << 6;    // Special parameter
    pub const LOCAL: u32 = 1 << 7;      // Local to function
    pub const EXPORT: u32 = 1 << 8;     // Exported to environment
    pub const UNSET: u32 = 1 << 9;      // Not yet set
    pub const TIED: u32 = 1 << 10;      // Tied to another param
    pub const UNIQUE: u32 = 1 << 11;    // Array elements unique
    pub const LOWER: u32 = 1 << 12;     // Lowercase value
    pub const UPPER: u32 = 1 << 13;     // Uppercase value
    pub const TAG: u32 = 1 << 14;       // Tagged parameter
    pub const HIDE: u32 = 1 << 15;      // Hidden
    pub const HIDEVAL: u32 = 1 << 16;   // Hide value
    pub const NORESTORE: u32 = 1 << 17; // Don't restore after function
}

/// Parameter value types
#[derive(Clone, Debug)]
pub enum ParamValue {
    Scalar(String),
    Integer(i64),
    Float(f64),
    Array(Vec<String>),
    Assoc(HashMap<String, String>),
    Unset,
}

impl Default for ParamValue {
    fn default() -> Self {
        ParamValue::Unset
    }
}

impl ParamValue {
    pub fn as_string(&self) -> String {
        match self {
            ParamValue::Scalar(s) => s.clone(),
            ParamValue::Integer(i) => i.to_string(),
            ParamValue::Float(f) => f.to_string(),
            ParamValue::Array(a) => a.join(" "),
            ParamValue::Assoc(h) => h.values().cloned().collect::<Vec<_>>().join(" "),
            ParamValue::Unset => String::new(),
        }
    }

    pub fn as_integer(&self) -> i64 {
        match self {
            ParamValue::Scalar(s) => s.parse().unwrap_or(0),
            ParamValue::Integer(i) => *i,
            ParamValue::Float(f) => *f as i64,
            ParamValue::Array(a) => a.len() as i64,
            ParamValue::Assoc(h) => h.len() as i64,
            ParamValue::Unset => 0,
        }
    }

    pub fn as_float(&self) -> f64 {
        match self {
            ParamValue::Scalar(s) => s.parse().unwrap_or(0.0),
            ParamValue::Integer(i) => *i as f64,
            ParamValue::Float(f) => *f,
            ParamValue::Array(a) => a.len() as f64,
            ParamValue::Assoc(h) => h.len() as f64,
            ParamValue::Unset => 0.0,
        }
    }

    pub fn as_array(&self) -> Vec<String> {
        match self {
            ParamValue::Scalar(s) => vec![s.clone()],
            ParamValue::Integer(i) => vec![i.to_string()],
            ParamValue::Float(f) => vec![f.to_string()],
            ParamValue::Array(a) => a.clone(),
            ParamValue::Assoc(h) => h.values().cloned().collect(),
            ParamValue::Unset => Vec::new(),
        }
    }

    pub fn is_set(&self) -> bool {
        !matches!(self, ParamValue::Unset)
    }
}

/// A shell parameter
#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub value: ParamValue,
    pub flags: u32,
    pub base: i32,       // Output base for integers
    pub width: i32,      // Output field width
    pub level: i32,      // Scope level
    pub ename: Option<String>, // Environment name (for tied params)
}

impl Param {
    pub fn new_scalar(name: &str, value: &str) -> Self {
        Param {
            name: name.to_string(),
            value: ParamValue::Scalar(value.to_string()),
            flags: flags::SCALAR,
            base: 10,
            width: 0,
            level: 0,
            ename: None,
        }
    }

    pub fn new_integer(name: &str, value: i64) -> Self {
        Param {
            name: name.to_string(),
            value: ParamValue::Integer(value),
            flags: flags::INTEGER,
            base: 10,
            width: 0,
            level: 0,
            ename: None,
        }
    }

    pub fn new_float(name: &str, value: f64) -> Self {
        Param {
            name: name.to_string(),
            value: ParamValue::Float(value),
            flags: flags::FLOAT,
            base: 10,
            width: 0,
            level: 0,
            ename: None,
        }
    }

    pub fn new_array(name: &str, value: Vec<String>) -> Self {
        Param {
            name: name.to_string(),
            value: ParamValue::Array(value),
            flags: flags::ARRAY,
            base: 10,
            width: 0,
            level: 0,
            ename: None,
        }
    }

    pub fn new_assoc(name: &str, value: HashMap<String, String>) -> Self {
        Param {
            name: name.to_string(),
            value: ParamValue::Assoc(value),
            flags: flags::ASSOC,
            base: 10,
            width: 0,
            level: 0,
            ename: None,
        }
    }

    pub fn is_readonly(&self) -> bool {
        (self.flags & flags::READONLY) != 0
    }

    pub fn is_exported(&self) -> bool {
        (self.flags & flags::EXPORT) != 0
    }

    pub fn is_local(&self) -> bool {
        (self.flags & flags::LOCAL) != 0
    }

    pub fn is_special(&self) -> bool {
        (self.flags & flags::SPECIAL) != 0
    }

    pub fn is_integer(&self) -> bool {
        (self.flags & flags::INTEGER) != 0
    }

    pub fn is_float(&self) -> bool {
        (self.flags & flags::FLOAT) != 0
    }

    pub fn is_array(&self) -> bool {
        (self.flags & flags::ARRAY) != 0
    }

    pub fn is_assoc(&self) -> bool {
        (self.flags & flags::ASSOC) != 0
    }
}

/// Parameter table
pub struct ParamTable {
    params: HashMap<String, Param>,
    local_level: i32,
    shtimer: u64, // Shell start time for $SECONDS
}

impl Default for ParamTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ParamTable {
    pub fn new() -> Self {
        let shtimer = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut table = ParamTable {
            params: HashMap::new(),
            local_level: 0,
            shtimer,
        };

        // Initialize special parameters
        table.init_special_params();
        
        table
    }

    fn init_special_params(&mut self) {
        // $$ - PID
        let pid = std::process::id() as i64;
        self.set_special("$", ParamValue::Integer(pid), flags::INTEGER | flags::READONLY | flags::SPECIAL);

        // $PPID
        #[cfg(unix)]
        {
            let ppid = unsafe { libc::getppid() } as i64;
            self.set_special("PPID", ParamValue::Integer(ppid), flags::INTEGER | flags::READONLY | flags::SPECIAL);
        }

        // $UID
        #[cfg(unix)]
        {
            let uid = unsafe { libc::getuid() } as i64;
            self.set_special("UID", ParamValue::Integer(uid), flags::INTEGER | flags::SPECIAL);
        }

        // $EUID
        #[cfg(unix)]
        {
            let euid = unsafe { libc::geteuid() } as i64;
            self.set_special("EUID", ParamValue::Integer(euid), flags::INTEGER | flags::SPECIAL);
        }

        // $GID
        #[cfg(unix)]
        {
            let gid = unsafe { libc::getgid() } as i64;
            self.set_special("GID", ParamValue::Integer(gid), flags::INTEGER | flags::SPECIAL);
        }

        // $EGID
        #[cfg(unix)]
        {
            let egid = unsafe { libc::getegid() } as i64;
            self.set_special("EGID", ParamValue::Integer(egid), flags::INTEGER | flags::SPECIAL);
        }

        // $SHLVL - incremented for each shell
        let shlvl = env::var("SHLVL")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0) + 1;
        self.set_special("SHLVL", ParamValue::Integer(shlvl), flags::INTEGER | flags::SPECIAL | flags::EXPORT);

        // $RANDOM - will be computed on access
        self.set_special("RANDOM", ParamValue::Integer(0), flags::INTEGER | flags::SPECIAL);

        // $LINENO
        self.set_special("LINENO", ParamValue::Integer(1), flags::INTEGER | flags::SPECIAL);

        // $? - last exit status
        self.set_special("?", ParamValue::Integer(0), flags::INTEGER | flags::READONLY | flags::SPECIAL);

        // $# - positional parameter count
        self.set_special("#", ParamValue::Integer(0), flags::INTEGER | flags::READONLY | flags::SPECIAL);

        // $! - last background job PID
        self.set_special("!", ParamValue::Integer(0), flags::INTEGER | flags::READONLY | flags::SPECIAL);

        // Import environment variables
        for (key, value) in env::vars() {
            if !self.params.contains_key(&key) {
                let mut param = Param::new_scalar(&key, &value);
                param.flags |= flags::EXPORT;
                self.params.insert(key, param);
            }
        }
    }

    fn set_special(&mut self, name: &str, value: ParamValue, pm_flags: u32) {
        let param = Param {
            name: name.to_string(),
            value,
            flags: pm_flags,
            base: 10,
            width: 0,
            level: 0,
            ename: None,
        };
        self.params.insert(name.to_string(), param);
    }

    /// Get a parameter value
    pub fn get(&self, name: &str) -> Option<&ParamValue> {
        // Handle special dynamic parameters
        match name {
            "RANDOM" => {
                // Return a pseudo-random value - actual implementation would update the param
                None // Let caller handle RANDOM specially
            }
            "SECONDS" => {
                // Return elapsed seconds since shell start
                None // Let caller handle SECONDS specially
            }
            _ => self.params.get(name).map(|p| &p.value),
        }
    }

    /// Get the full parameter
    pub fn get_param(&self, name: &str) -> Option<&Param> {
        self.params.get(name)
    }

    /// Get mutable parameter
    pub fn get_param_mut(&mut self, name: &str) -> Option<&mut Param> {
        self.params.get_mut(name)
    }

    /// Set a scalar parameter
    pub fn set_scalar(&mut self, name: &str, value: &str) -> bool {
        if let Some(param) = self.params.get_mut(name) {
            if param.is_readonly() {
                return false;
            }
            // Apply transformations
            let value = if (param.flags & flags::LOWER) != 0 {
                value.to_lowercase()
            } else if (param.flags & flags::UPPER) != 0 {
                value.to_uppercase()
            } else {
                value.to_string()
            };
            param.value = ParamValue::Scalar(value);
            
            // Update environment if exported
            if param.is_exported() {
                env::set_var(name, param.value.as_string());
            }
            true
        } else {
            let param = Param::new_scalar(name, value);
            self.params.insert(name.to_string(), param);
            true
        }
    }

    /// Set an integer parameter
    pub fn set_integer(&mut self, name: &str, value: i64) -> bool {
        if let Some(param) = self.params.get_mut(name) {
            if param.is_readonly() {
                return false;
            }
            param.value = ParamValue::Integer(value);
            if param.is_exported() {
                env::set_var(name, value.to_string());
            }
            true
        } else {
            let param = Param::new_integer(name, value);
            self.params.insert(name.to_string(), param);
            true
        }
    }

    /// Set an array parameter
    pub fn set_array(&mut self, name: &str, value: Vec<String>) -> bool {
        if let Some(param) = self.params.get_mut(name) {
            if param.is_readonly() {
                return false;
            }
            let value = if (param.flags & flags::UNIQUE) != 0 {
                // Remove duplicates while preserving order
                let mut seen = std::collections::HashSet::new();
                value.into_iter().filter(|s| seen.insert(s.clone())).collect()
            } else {
                value
            };
            param.value = ParamValue::Array(value);
            true
        } else {
            let param = Param::new_array(name, value);
            self.params.insert(name.to_string(), param);
            true
        }
    }

    /// Set an associative array parameter
    pub fn set_assoc(&mut self, name: &str, value: HashMap<String, String>) -> bool {
        if let Some(param) = self.params.get_mut(name) {
            if param.is_readonly() {
                return false;
            }
            param.value = ParamValue::Assoc(value);
            true
        } else {
            let param = Param::new_assoc(name, value);
            self.params.insert(name.to_string(), param);
            true
        }
    }

    /// Unset a parameter
    pub fn unset(&mut self, name: &str) -> bool {
        if let Some(param) = self.params.get(name) {
            if param.is_readonly() {
                return false;
            }
        }
        self.params.remove(name);
        env::remove_var(name);
        true
    }

    /// Export a parameter
    pub fn export(&mut self, name: &str) -> bool {
        if let Some(param) = self.params.get_mut(name) {
            param.flags |= flags::EXPORT;
            env::set_var(name, param.value.as_string());
            true
        } else {
            false
        }
    }

    /// Mark parameter as readonly
    pub fn set_readonly(&mut self, name: &str) -> bool {
        if let Some(param) = self.params.get_mut(name) {
            param.flags |= flags::READONLY;
            true
        } else {
            false
        }
    }

    /// Start a new local scope
    pub fn push_scope(&mut self) {
        self.local_level += 1;
    }

    /// End a local scope
    pub fn pop_scope(&mut self) {
        // Remove local variables from the scope being popped
        self.params.retain(|_, param| {
            !param.is_local() || param.level < self.local_level
        });
        self.local_level -= 1;
    }

    /// Create a local variable
    pub fn make_local(&mut self, name: &str) {
        if let Some(param) = self.params.get_mut(name) {
            param.flags |= flags::LOCAL;
            param.level = self.local_level;
        }
    }

    /// Get the SECONDS value
    pub fn get_seconds(&self) -> f64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        (now - self.shtimer) as f64
    }

    /// Get a random value (updates internal state)
    pub fn get_random(&self) -> i64 {
        use std::time::Instant;
        // Simple random based on current time - real implementation would use proper PRNG
        let now = Instant::now();
        (now.elapsed().as_nanos() % 32768) as i64
    }

    /// Iterate over all parameters
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Param)> {
        self.params.iter()
    }

    /// Check if a parameter exists
    pub fn contains(&self, name: &str) -> bool {
        self.params.contains_key(name)
    }
}

/// Colon-separated path to array
pub fn colonarr_to_array(s: &str) -> Vec<String> {
    s.split(':')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Array to colon-separated path
pub fn array_to_colonarr(arr: &[String]) -> String {
    arr.join(":")
}

/// Subscript index result from getindex
/// Port from zsh params.c Value struct's start/end fields
#[derive(Debug, Clone)]
pub struct SubscriptIndex {
    pub start: i64,
    pub end: i64,
    pub is_all: bool,  // True for @ or *
}

impl SubscriptIndex {
    pub fn single(idx: i64) -> Self {
        SubscriptIndex {
            start: idx,
            end: idx + 1,
            is_all: false,
        }
    }

    pub fn range(start: i64, end: i64) -> Self {
        SubscriptIndex {
            start,
            end,
            is_all: false,
        }
    }

    pub fn all() -> Self {
        SubscriptIndex {
            start: 0,
            end: -1,
            is_all: true,
        }
    }
}

/// Parse a subscript expression like "[1]", "[1,5]", "[@]", "[*]"
/// Port from zsh/Src/params.c getindex()
///
/// Returns the subscript index with start and end positions.
/// For zsh, arrays are 1-indexed by default unless KSH_ARRAYS is set.
pub fn parse_subscript(subscript: &str, ksh_arrays: bool) -> Option<SubscriptIndex> {
    let s = subscript.trim();
    
    // Handle @ and * for all elements
    if s == "@" || s == "*" {
        return Some(SubscriptIndex::all());
    }
    
    // Check for range notation: start,end
    if let Some(comma_pos) = s.find(',') {
        let start_str = s[..comma_pos].trim();
        let end_str = s[comma_pos + 1..].trim();
        
        let start = parse_index_value(start_str, ksh_arrays)?;
        let end = parse_index_value(end_str, ksh_arrays)?;
        
        return Some(SubscriptIndex::range(start, end));
    }
    
    // Single index
    let idx = parse_index_value(s, ksh_arrays)?;
    Some(SubscriptIndex::single(idx))
}

/// Parse a single index value, handling negative indices
/// Port from zsh/Src/params.c getarg()
fn parse_index_value(s: &str, ksh_arrays: bool) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    
    // Try parsing as integer
    if let Ok(idx) = s.parse::<i64>() {
        // In zsh (non-KSH mode), adjust 1-indexed to 0-indexed internally
        // but return as-is since caller will handle indexing
        if !ksh_arrays && idx > 0 {
            // Keep as 1-indexed for the caller
        }
        return Some(idx);
    }
    
    // Could be an arithmetic expression - for now just fail
    None
}

/// Get array slice based on subscript index
/// Port from zsh array access logic in params.c
pub fn get_array_slice(arr: &[String], idx: &SubscriptIndex, ksh_arrays: bool) -> Vec<String> {
    if idx.is_all {
        return arr.to_vec();
    }
    
    let len = arr.len() as i64;
    
    // Convert indices (zsh is 1-indexed, arrays are 0-indexed internally)
    let start = if idx.start < 0 {
        // Negative index counts from end
        (len + idx.start).max(0) as usize
    } else if ksh_arrays {
        // KSH_ARRAYS: 0-indexed
        idx.start as usize
    } else {
        // zsh default: 1-indexed, convert to 0-indexed
        if idx.start > 0 {
            (idx.start - 1) as usize
        } else {
            0
        }
    };
    
    let end = if idx.end < 0 {
        // Negative index counts from end
        ((len + idx.end + 1).max(0) as usize).min(arr.len())
    } else if ksh_arrays {
        // KSH_ARRAYS: 0-indexed, end is exclusive
        (idx.end as usize).min(arr.len())
    } else {
        // zsh default: 1-indexed, end is inclusive
        (idx.end as usize).min(arr.len())
    };
    
    if start >= arr.len() || start >= end {
        return Vec::new();
    }
    
    arr[start..end].to_vec()
}

/// Get single array element by index
/// Port from zsh array access in params.c
pub fn get_array_element(arr: &[String], idx: i64, ksh_arrays: bool) -> Option<String> {
    let len = arr.len() as i64;
    
    let actual_idx = if idx < 0 {
        // Negative index counts from end
        let adj = len + idx;
        if adj < 0 {
            return None;
        }
        adj as usize
    } else if ksh_arrays {
        // KSH_ARRAYS: 0-indexed
        idx as usize
    } else {
        // zsh default: 1-indexed
        if idx > 0 {
            (idx - 1) as usize
        } else {
            return None;
        }
    };
    
    arr.get(actual_idx).cloned()
}

/// Get integer parameter value (from params.c getiparam lines 3043-3052)
pub fn getiparam(table: &ParamTable, name: &str) -> i64 {
    table.get(name).map(|v| v.as_integer()).unwrap_or(0)
}

/// Get scalar (string) parameter (from params.c getsparam lines 3075-3084)
pub fn getsparam(table: &ParamTable, name: &str) -> Option<String> {
    table.get(name).map(|v| v.as_string())
}

/// Get array parameter (from params.c getaparam lines 3099-3109)
pub fn getaparam(table: &ParamTable, name: &str) -> Option<Vec<String>> {
    match table.get(name) {
        Some(ParamValue::Array(arr)) => Some(arr.clone()),
        _ => None,
    }
}

/// Get hash parameter values as array (from params.c gethparam lines 3114-3124)
pub fn gethparam(table: &ParamTable, name: &str) -> Option<Vec<String>> {
    match table.get(name) {
        Some(ParamValue::Assoc(h)) => Some(h.values().cloned().collect()),
        _ => None,
    }
}

/// Get hash parameter keys as array (from params.c gethkparam lines 3129-3139)
pub fn gethkparam(table: &ParamTable, name: &str) -> Option<Vec<String>> {
    match table.get(name) {
        Some(ParamValue::Assoc(h)) => Some(h.keys().cloned().collect()),
        _ => None,
    }
}

/// Numeric type for parameters (from params.c mnumber)
#[derive(Clone, Debug)]
pub enum MNumber {
    Integer(i64),
    Float(f64),
}

impl Default for MNumber {
    fn default() -> Self {
        MNumber::Integer(0)
    }
}

/// Get numeric parameter (from params.c getnparam lines 3057-3070)
pub fn getnparam(table: &ParamTable, name: &str) -> MNumber {
    match table.get(name) {
        Some(ParamValue::Integer(i)) => MNumber::Integer(*i),
        Some(ParamValue::Float(f)) => MNumber::Float(*f),
        Some(ParamValue::Scalar(s)) => {
            if let Ok(i) = s.parse::<i64>() {
                MNumber::Integer(i)
            } else if let Ok(f) = s.parse::<f64>() {
                MNumber::Float(f)
            } else {
                MNumber::default()
            }
        }
        _ => MNumber::default(),
    }
}

/// Assign string parameter (from params.c assignsparam lines 3192-3300)
pub fn assignsparam(table: &mut ParamTable, name: &str, val: &str) -> bool {
    table.set_scalar(name, val)
}

/// Assign integer parameter (from params.c assigniparam)
pub fn assigniparam(table: &mut ParamTable, name: &str, val: i64) -> bool {
    table.set_integer(name, val)
}

/// Assign array parameter (from params.c assignaparam)
pub fn assignaparam(table: &mut ParamTable, name: &str, val: Vec<String>) -> bool {
    table.set_array(name, val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_param_value_conversions() {
        let scalar = ParamValue::Scalar("42".to_string());
        assert_eq!(scalar.as_integer(), 42);
        assert_eq!(scalar.as_float(), 42.0);
        assert_eq!(scalar.as_string(), "42");
    }

    #[test]
    fn test_param_table_set_get() {
        let mut table = ParamTable::new();
        table.set_scalar("FOO", "bar");
        
        let value = table.get("FOO").unwrap();
        assert_eq!(value.as_string(), "bar");
    }

    #[test]
    fn test_param_readonly() {
        let mut table = ParamTable::new();
        table.set_scalar("TEST", "value");
        table.set_readonly("TEST");
        
        assert!(!table.set_scalar("TEST", "new_value"));
        assert_eq!(table.get("TEST").unwrap().as_string(), "value");
    }

    #[test]
    fn test_param_array() {
        let mut table = ParamTable::new();
        table.set_array("arr", vec!["a".into(), "b".into(), "c".into()]);
        
        let value = table.get("arr").unwrap();
        assert_eq!(value.as_array(), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_param_assoc() {
        let mut table = ParamTable::new();
        let mut hash = HashMap::new();
        hash.insert("key".to_string(), "value".to_string());
        table.set_assoc("hash", hash);
        
        if let ParamValue::Assoc(h) = table.get("hash").unwrap() {
            assert_eq!(h.get("key"), Some(&"value".to_string()));
        } else {
            panic!("Expected associative array");
        }
    }

    #[test]
    fn test_colonarr_conversion() {
        let arr = colonarr_to_array("/bin:/usr/bin:/usr/local/bin");
        assert_eq!(arr, vec!["/bin", "/usr/bin", "/usr/local/bin"]);
        
        let path = array_to_colonarr(&arr);
        assert_eq!(path, "/bin:/usr/bin:/usr/local/bin");
    }

    #[test]
    fn test_local_scope() {
        let mut table = ParamTable::new();
        table.set_scalar("GLOBAL", "value");
        
        table.push_scope();
        table.set_scalar("LOCAL", "local_value");
        table.make_local("LOCAL");
        
        assert!(table.contains("LOCAL"));
        
        table.pop_scope();
        assert!(!table.contains("LOCAL"));
        assert!(table.contains("GLOBAL"));
    }
}
