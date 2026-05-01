//! Source-level desugar for `tool fn` and `mcp_server` syntax.
//!
//! Rather than extending the parser AST + interpreter to special-case
//! these forms, we run a pre-pass that rewrites them into ordinary
//! stryke source the regular parser already understands.
//!
//!   tool fn weather($city: string, $units: string)
//!       "Get current weather for a city" {
//!       fetch "<https://api.weather.com/>" . uri_encode($city)
//!   }
//!
//! desugars to:
//!
//!   fn weather($city, $units) {
//!       fetch "<https://api.weather.com/>" . uri_encode($city)
//!   }
//!   ai_register_tool("weather", "Get current weather for a city",
//!       +{ city => "string", units => "string" }, \&weather);
//!
//! And:
//!
//!   mcp_server "filesystem" {
//!       tool read_file($path: string) "Read file" {
//!           slurp $path
//!       }
//!       tool list_dir($path: string) "List dir" {
//!           join("\n", readdir $path)
//!       }
//!   }
//!
//! desugars to:
//!
//!   {
//!       fn _mcp_read_file_..($path) { slurp $path }
//!       fn _mcp_list_dir_..($path) { join("\n", readdir $path) }
//!       mcp_server_start("filesystem", +{
//!           tools => [
//!               +{ name => "read_file", description => "Read file",
//!                  parameters => +{ path => "string" },
//!                  run => \&_mcp_read_file_.. },
//!               +{ name => "list_dir", description => "List dir",
//!                  parameters => +{ path => "string" },
//!                  run => \&_mcp_list_dir_.. },
//!           ]
//!       });
//!   }

pub fn desugar(code: &str) -> String {
    let mut out = code.to_string();
    if out.contains("tool fn") || out.contains("tool\tfn") {
        out = desugar_tool_fn(&out);
    }
    if out.contains("mcp_server") {
        out = desugar_mcp_server(&out);
    }
    out
}

// ── tool fn ───────────────────────────────────────────────────────────

pub(crate) fn desugar_tool_fn(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    let mut can_start_stmt = true;
    while i < bytes.len() {
        let c = bytes[i];
        // Skip over strings / regex / comments — same shape as the
        // existing `desugar_rust_blocks`.
        match c {
            b'\n' | b' ' | b'\t' | b'\r' => {
                out.push(c as char);
                i += 1;
                continue;
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out.push(bytes[i] as char);
                    i += 1;
                }
                continue;
            }
            b'"' | b'\'' | b'`' => {
                let q = c;
                out.push(c as char);
                i += 1;
                while i < bytes.len() {
                    let b = bytes[i];
                    if b == b'\\' && i + 1 < bytes.len() {
                        out.push(b as char);
                        out.push(bytes[i + 1] as char);
                        i += 2;
                        continue;
                    }
                    out.push(b as char);
                    i += 1;
                    if b == q {
                        break;
                    }
                }
                continue;
            }
            _ => {}
        }
        if can_start_stmt && matches_word(bytes, i, b"tool") {
            // Need whitespace then `fn` to match.
            let after = i + 4;
            let mut k = after;
            while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                k += 1;
            }
            if matches_word(bytes, k, b"fn") {
                // Try to parse the full tool fn shape; fall back to passthrough
                // on error (e.g. mid-expression "tool fn" reference).
                if let Some((replacement, end)) = try_parse_tool_fn(bytes, i) {
                    out.push_str(&replacement);
                    i = end;
                    continue;
                }
            }
        }
        if c == b';' || c == b'{' || c == b'}' {
            can_start_stmt = true;
            out.push(c as char);
            i += 1;
            continue;
        }
        out.push(c as char);
        i += 1;
        if !c.is_ascii_whitespace() {
            can_start_stmt = false;
        }
    }
    out
}

fn matches_word(bytes: &[u8], i: usize, word: &[u8]) -> bool {
    if i + word.len() > bytes.len() {
        return false;
    }
    if &bytes[i..i + word.len()] != word {
        return false;
    }
    let next = bytes.get(i + word.len()).copied().unwrap_or(0);
    !next.is_ascii_alphanumeric() && next != b'_'
}

fn try_parse_tool_fn(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    // Cursor: after `tool`, after `fn`, at name.
    let mut i = start + 4;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if !matches_word(bytes, i, b"fn") {
        return None;
    }
    i += 2;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    // Function name.
    let name_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = std::str::from_utf8(&bytes[name_start..i]).ok()?.to_string();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    // Optional parameter list (...).
    let (params_text, params_specs) = if i < bytes.len() && bytes[i] == b'(' {
        let close = find_matching(bytes, i, b'(', b')')?;
        let inside = std::str::from_utf8(&bytes[i + 1..close]).ok()?.to_string();
        i = close + 1;
        let (stripped, specs) = parse_param_list(&inside);
        (format!("({})", stripped), specs)
    } else {
        (String::new(), Vec::new())
    };
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    // Optional return type `-> Type`.
    if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'>' {
        i += 2;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        // Skip the type identifier (alphanumeric/_/<>::).
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric()
                || bytes[i] == b'_'
                || bytes[i] == b':'
                || bytes[i] == b'<'
                || bytes[i] == b'>')
        {
            i += 1;
        }
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
    }
    // Optional docstring (double-quoted only for the desugar — single quotes
    // get awkward to embed in the generated code).
    let mut doc = String::new();
    if i < bytes.len() && bytes[i] == b'"' {
        let close = find_quote_close(bytes, i)?;
        doc = std::str::from_utf8(&bytes[i + 1..close]).ok()?.to_string();
        i = close + 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
    }
    // Body.
    if i >= bytes.len() || bytes[i] != b'{' {
        return None;
    }
    let body_close = find_matching(bytes, i, b'{', b'}')?;
    let body = std::str::from_utf8(&bytes[i..=body_close])
        .ok()?
        .to_string();
    let end = body_close + 1;

    let params_hash = build_params_hash(&params_specs);
    let doc_lit = escape_double_quoted(&doc);
    // The agent loop calls the tool with a single hashref of args.
    // Wrap the body so the named params from the signature are
    // visible as locals — `tool fn weather($city)` makes `$city`
    // available inside the body, just like the user's mental model.
    let wrapped_body = build_param_unpack(&params_specs, &body);
    let replacement = format!(
        "fn {name} {body}; ai_register_tool(\"{name}\", \"{doc}\", {params_hash}, \\&{name})",
        name = name,
        body = wrapped_body,
        doc = doc_lit,
        params_hash = params_hash,
    );
    let _ = params_text;
    Some((replacement, end))
}

fn build_param_unpack(specs: &[(String, String)], body: &str) -> String {
    if specs.is_empty() {
        return body.to_string();
    }
    let mut prelude = String::new();
    prelude.push_str("my $__args__ = $_[0]; ");
    for (name, _) in specs {
        prelude.push_str(&format!("my ${} = $__args__->{{{}}}; ", name, name));
    }
    // Inject prelude after the opening brace.
    if let Some(idx) = body.find('{') {
        let mut out = String::with_capacity(body.len() + prelude.len() + 2);
        out.push_str(&body[..=idx]);
        out.push(' ');
        out.push_str(&prelude);
        out.push_str(&body[idx + 1..]);
        return out;
    }
    body.to_string()
}

fn parse_param_list(src: &str) -> (String, Vec<(String, String)>) {
    // Split on commas at depth 0, then strip `: Type` from each.
    let bytes = src.as_bytes();
    let mut parts: Vec<String> = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(src[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < src.len() {
        parts.push(src[start..].to_string());
    }
    let mut specs: Vec<(String, String)> = Vec::new();
    let mut stripped_parts: Vec<String> = Vec::new();
    for raw in parts {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Split off `: Type` (also tolerates ` : Type` with spaces, but
        // not `?:` / `::` — we look for a leading `: ` after the var).
        let (binding, ty) = if let Some(idx) = find_param_colon(trimmed) {
            let (l, r) = trimmed.split_at(idx);
            (l.trim().to_string(), r[1..].trim().to_string())
        } else {
            (trimmed.to_string(), "Any".to_string())
        };
        // Strip default-value tail to keep stryke fn happy with our
        // generated fn signature carrying defaults — but stryke already
        // supports defaults, so we keep them.
        stripped_parts.push(binding.clone());
        // Param name without sigil for the schema map.
        let name_only = binding
            .trim_start_matches('$')
            .trim_start_matches('@')
            .trim_start_matches('%')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .split('=')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        specs.push((name_only, normalize_type(&ty)));
    }
    (stripped_parts.join(", "), specs)
}

fn find_param_colon(s: &str) -> Option<usize> {
    // Find the first `:` that isn't part of `::` and isn't inside a
    // bracketed group — that's the type-annotation colon.
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b':' if depth == 0 => {
                if i + 1 < bytes.len() && bytes[i + 1] == b':' {
                    i += 2;
                    continue;
                }
                if i > 0 && bytes[i - 1] == b':' {
                    i += 1;
                    continue;
                }
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn normalize_type(t: &str) -> String {
    let lower = t.trim().to_ascii_lowercase();
    let canonical = match lower.as_str() {
        "string" | "str" => "string",
        "int" | "integer" | "i64" => "int",
        "float" | "number" | "num" | "f64" => "number",
        "bool" | "boolean" => "bool",
        "array" | "list" | "arrayref" => "array",
        "hash" | "hashref" | "object" => "object",
        _ => "string",
    };
    canonical.to_string()
}

fn build_params_hash(specs: &[(String, String)]) -> String {
    if specs.is_empty() {
        return "+{}".to_string();
    }
    let pairs: Vec<String> = specs
        .iter()
        .map(|(k, v)| format!("{} => \"{}\"", k, v))
        .collect();
    format!("+{{ {} }}", pairs.join(", "))
}

fn find_matching(bytes: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    if bytes.get(start) != Some(&open) {
        return None;
    }
    let mut depth = 0i32;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' | b'\'' | b'`' => {
                let q = c;
                i += 1;
                while i < bytes.len() {
                    let b = bytes[i];
                    if b == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    if b == q {
                        break;
                    }
                }
                continue;
            }
            x if x == open => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_quote_close(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'"') {
        return None;
    }
    let mut i = start + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if b == b'"' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn escape_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '$' => out.push_str("\\$"),
            '@' => out.push_str("\\@"),
            other => out.push(other),
        }
    }
    out
}

// ── mcp_server "name" { ... } ────────────────────────────────────────

fn desugar_mcp_server(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    let mut can_start_stmt = true;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\n' | b' ' | b'\t' | b'\r' => {
                out.push(c as char);
                i += 1;
                continue;
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out.push(bytes[i] as char);
                    i += 1;
                }
                continue;
            }
            b'"' | b'\'' | b'`' => {
                let q = c;
                out.push(c as char);
                i += 1;
                while i < bytes.len() {
                    let b = bytes[i];
                    if b == b'\\' && i + 1 < bytes.len() {
                        out.push(b as char);
                        out.push(bytes[i + 1] as char);
                        i += 2;
                        continue;
                    }
                    out.push(b as char);
                    i += 1;
                    if b == q {
                        break;
                    }
                }
                continue;
            }
            _ => {}
        }
        if can_start_stmt && matches_word(bytes, i, b"mcp_server") {
            // Look for `mcp_server "name" {`.
            let mut k = i + 10;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k < bytes.len() && bytes[k] == b'"' {
                if let Some(qclose) = find_quote_close(bytes, k) {
                    let name = std::str::from_utf8(&bytes[k + 1..qclose])
                        .ok()
                        .map(|s| s.to_string());
                    let mut bk = qclose + 1;
                    while bk < bytes.len() && bytes[bk].is_ascii_whitespace() {
                        bk += 1;
                    }
                    if bk < bytes.len() && bytes[bk] == b'{' {
                        if let Some(end) = find_matching(bytes, bk, b'{', b'}') {
                            if let Some(name) = name {
                                let body = std::str::from_utf8(&bytes[bk + 1..end]).unwrap_or("");
                                let replacement = build_mcp_server_call(&name, body);
                                out.push_str(&replacement);
                                i = end + 1;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        if c == b';' || c == b'{' || c == b'}' {
            can_start_stmt = true;
            out.push(c as char);
            i += 1;
            continue;
        }
        out.push(c as char);
        i += 1;
        if !c.is_ascii_whitespace() {
            can_start_stmt = false;
        }
    }
    out
}

fn build_mcp_server_call(name: &str, body: &str) -> String {
    // Inside the body, look for `tool TOOLNAME(...) "doc" { ... }` blocks.
    let bytes = body.as_bytes();
    let mut decls = Vec::<String>::new();
    let mut tool_specs = Vec::<String>::new();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // Comment.
        if bytes[i] == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if !matches_word(bytes, i, b"tool") {
            break;
        }
        // Found a tool — parse: tool NAME(params) [-> Type] "doc" { body }
        i += 4;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i == name_start {
            break;
        }
        let tool_name = std::str::from_utf8(&bytes[name_start..i])
            .unwrap_or("")
            .to_string();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let (params_text, specs) = if i < bytes.len() && bytes[i] == b'(' {
            let close = match find_matching(bytes, i, b'(', b')') {
                Some(c) => c,
                None => break,
            };
            let inside = std::str::from_utf8(&bytes[i + 1..close]).unwrap_or("");
            i = close + 1;
            let (stripped, specs) = parse_param_list(inside);
            (format!("({})", stripped), specs)
        } else {
            (String::new(), Vec::new())
        };
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'>' {
            i += 2;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || bytes[i] == b'_'
                    || bytes[i] == b':'
                    || bytes[i] == b'<'
                    || bytes[i] == b'>')
            {
                i += 1;
            }
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        }
        let mut doc = String::new();
        if i < bytes.len() && bytes[i] == b'"' {
            let close = match find_quote_close(bytes, i) {
                Some(c) => c,
                None => break,
            };
            doc = std::str::from_utf8(&bytes[i + 1..close])
                .unwrap_or("")
                .to_string();
            i = close + 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        }
        if i >= bytes.len() || bytes[i] != b'{' {
            break;
        }
        let body_close = match find_matching(bytes, i, b'{', b'}') {
            Some(c) => c,
            None => break,
        };
        let body_text = std::str::from_utf8(&bytes[i..=body_close]).unwrap_or("");
        i = body_close + 1;

        // Synth a unique helper name so we don't leak it in user scope.
        let helper = format!(
            "_mcp_{}_{}_{}",
            sanitize(name),
            sanitize(&tool_name),
            decls.len()
        );
        let wrapped = build_param_unpack(&specs, body_text);
        let _ = params_text;
        decls.push(format!("fn {} {}", helper, wrapped));
        let params_hash = build_params_hash(&specs);
        let doc_lit = escape_double_quoted(&doc);
        tool_specs.push(format!(
            "+{{ name => \"{}\", description => \"{}\", parameters => {}, run => \\&{} }}",
            tool_name, doc_lit, params_hash, helper
        ));
        // Optional trailing semicolon between tools.
        while i < bytes.len() && (bytes[i] == b';' || bytes[i].is_ascii_whitespace()) {
            i += 1;
        }
    }

    let name_lit = escape_double_quoted(name);
    if tool_specs.is_empty() {
        return format!("mcp_server_start(\"{}\", +{{ tools => [] }});", name_lit);
    }
    format!(
        "{}\nmcp_server_start(\"{}\", +{{ tools => [{}] }})",
        decls.join("\n"),
        name_lit,
        tool_specs.join(",\n  ")
    )
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
