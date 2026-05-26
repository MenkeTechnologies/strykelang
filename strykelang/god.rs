//! `god EXPR` — omniscient runtime introspection.
//!
//! Renders a structured multi-line dump of a [`StrykeValue`] showing
//! information that `pp`/`ddump` do not surface: the canonical type tag,
//! the heap pointer (so two refs to the same heap object are visibly
//! aliases), the `Arc` strong / weak counts, an estimated payload size,
//! and per-variant detail (hex preview for byte buffers, generator state,
//! pipeline op count, closure captures, …).
//!
//! Reference cycles are detected by tracking the pointer of every heap
//! object on the current recursion path in a [`HashSet`]; a back-edge to
//! an already-visited pointer is annotated `...cycle to 0x…` and not
//! re-descended, so `god` is safe to call on self-referential structures.
//!
//! Pure: returns the dump as a string. Callers that want stderr printing
//! pipe the result through `warn` or `p` explicitly.
//!
//! Sibling reflectors: `pp` is the human-friendly value formatter, `ddump`
//! is the deep value-structure dump. `god` is the runtime-metadata layer
//! that sits underneath both.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::sync::Arc;

use crate::value::StrykeValue;

const INDENT: &str = "    ";

/// Render a full god-mode dump of `v` as a string.
pub fn god_dump(v: &StrykeValue) -> String {
    let mut out = String::new();
    let mut visited: HashSet<usize> = HashSet::new();
    dump_into(&mut out, v, 0, &mut visited);
    out
}

fn dump_into(out: &mut String, v: &StrykeValue, depth: usize, visited: &mut HashSet<usize>) {
    let prefix = INDENT.repeat(depth);

    // Immediates — no heap, no refcount.
    if v.is_undef() {
        let _ = writeln!(out, "{prefix}undef");
        return;
    }
    if let Some(n) = v.as_integer() {
        let _ = writeln!(out, "{prefix}INTEGER {n} (immediate)");
        return;
    }
    if let Some(f) = v.as_float() {
        let bits = f.to_bits();
        let _ = writeln!(out, "{prefix}FLOAT {f} (bits=0x{bits:016x})");
        return;
    }

    let tag = v.type_name();

    // Byte buffer: hex preview + length, plus heap addr / refcount.
    if let Some(b) = v.as_bytes_arc() {
        let ptr = Arc::as_ptr(&b) as usize;
        let strong = Arc::strong_count(&b);
        let weak = Arc::weak_count(&b);
        let len = b.len();
        let preview = hex_preview(&b, 32);
        let _ = writeln!(
            out,
            "{prefix}BYTES @ 0x{ptr:x} (strong={strong}, weak={weak}, len={len}) hex={preview}"
        );
        return;
    }

    // Strings carry no shared Arc the way Bytes do; treat as an immediate-ish.
    if tag == "STRING" {
        let s = v.to_string();
        let _ = writeln!(
            out,
            "{prefix}STRING (len={}, bytes={}) {:?}",
            s.chars().count(),
            s.len(),
            short_string(&s, 80),
        );
        return;
    }

    // ArrayRef — recurse into elements under the shared lock.
    if let Some(arc) = v.as_array_ref() {
        let ptr = Arc::as_ptr(&arc) as usize;
        if !visited.insert(ptr) {
            let _ = writeln!(out, "{prefix}ARRAY @ 0x{ptr:x} ...cycle");
            return;
        }
        let strong = Arc::strong_count(&arc);
        let weak = Arc::weak_count(&arc);
        let guard = arc.read();
        let _ = writeln!(
            out,
            "{prefix}ARRAY @ 0x{ptr:x} (strong={strong}, weak={weak}, len={})",
            guard.len(),
        );
        for (i, elem) in guard.iter().enumerate() {
            let _ = writeln!(out, "{prefix}{INDENT}[{i}]");
            dump_into(out, elem, depth + 2, visited);
        }
        visited.remove(&ptr);
        return;
    }

    // HashRef — recurse into entries (preserves insertion order via IndexMap).
    if let Some(arc) = v.as_hash_ref() {
        let ptr = Arc::as_ptr(&arc) as usize;
        if !visited.insert(ptr) {
            let _ = writeln!(out, "{prefix}HASH @ 0x{ptr:x} ...cycle");
            return;
        }
        let strong = Arc::strong_count(&arc);
        let weak = Arc::weak_count(&arc);
        let guard = arc.read();
        let _ = writeln!(
            out,
            "{prefix}HASH @ 0x{ptr:x} (strong={strong}, weak={weak}, entries={})",
            guard.len(),
        );
        for (k, val) in guard.iter() {
            let _ = writeln!(out, "{prefix}{INDENT}{:?} =>", k);
            dump_into(out, val, depth + 2, visited);
        }
        visited.remove(&ptr);
        return;
    }

    // Plain Array (owned, not a ref): recurse without pointer tracking — the
    // cloned Vec has no shared identity, so it cannot participate in a cycle.
    if let Some(elems) = v.as_array_vec() {
        // Only treat as plain Array when this value is NOT also an ArrayRef
        // (which would've returned above) and the type tag claims ARRAY.
        if tag == "ARRAY" {
            let _ = writeln!(out, "{prefix}ARRAY (owned, len={})", elems.len());
            for (i, elem) in elems.iter().enumerate() {
                let _ = writeln!(out, "{prefix}{INDENT}[{i}]");
                dump_into(out, elem, depth + 2, visited);
            }
            return;
        }
    }

    // Plain Hash (owned, not a ref).
    if let Some(h) = v.as_hash_map() {
        if tag == "HASH" {
            let _ = writeln!(out, "{prefix}HASH (owned, entries={})", h.len());
            for (k, val) in h.iter() {
                let _ = writeln!(out, "{prefix}{INDENT}{:?} =>", k);
                dump_into(out, val, depth + 2, visited);
            }
            return;
        }
    }

    // Scalar ref / capture cell — dereference one level.
    if let Some(arc) = v.as_scalar_ref() {
        let ptr = Arc::as_ptr(&arc) as usize;
        if !visited.insert(ptr) {
            let _ = writeln!(out, "{prefix}SCALAR @ 0x{ptr:x} ...cycle");
            return;
        }
        let strong = Arc::strong_count(&arc);
        let weak = Arc::weak_count(&arc);
        let inner = arc.read().clone();
        let _ = writeln!(
            out,
            "{prefix}SCALAR ref @ 0x{ptr:x} (strong={strong}, weak={weak}) →",
        );
        dump_into(out, &inner, depth + 1, visited);
        visited.remove(&ptr);
        return;
    }

    // Code ref — show signature + closure-capture summary.
    if let Some(sub) = v.as_code_ref() {
        let ptr = Arc::as_ptr(&sub) as usize;
        let strong = Arc::strong_count(&sub);
        let weak = Arc::weak_count(&sub);
        let name = if sub.name.is_empty() {
            "<anon>".to_string()
        } else {
            sub.name.clone()
        };
        let proto = sub.prototype.clone().unwrap_or_else(|| "-".to_string());
        let params = sub.params.len();
        let body_stmts = sub.body.len();
        let captures = sub
            .closure_env
            .as_ref()
            .map(|env| env.len())
            .unwrap_or(0);
        let _ = writeln!(
            out,
            "{prefix}CODE @ 0x{ptr:x} (strong={strong}, weak={weak}) name={name} params={params} stmts={body_stmts} prototype={proto} captures={captures}"
        );
        if let Some(env) = &sub.closure_env {
            for (var_name, captured) in env {
                let _ = writeln!(out, "{prefix}{INDENT}capture {var_name} =>");
                dump_into(out, captured, depth + 2, visited);
            }
        }
        return;
    }

    // Generator — pc, exhausted, body length.
    if let Some(gen) = v.as_generator() {
        let ptr = Arc::as_ptr(&gen) as usize;
        let strong = Arc::strong_count(&gen);
        let weak = Arc::weak_count(&gen);
        let pc = *gen.pc.lock();
        let exhausted = *gen.exhausted.lock();
        let body = gen.block.len();
        let _ = writeln!(
            out,
            "{prefix}Generator @ 0x{ptr:x} (strong={strong}, weak={weak}) pc={pc}/{body} exhausted={exhausted}"
        );
        return;
    }

    // Pipeline — queued ops + source length, scalar-terminal flag.
    if let Some(pl) = v.as_pipeline() {
        let ptr = Arc::as_ptr(&pl) as usize;
        let strong = Arc::strong_count(&pl);
        let weak = Arc::weak_count(&pl);
        let inner = pl.lock();
        let _ = writeln!(
            out,
            "{prefix}Pipeline @ 0x{ptr:x} (strong={strong}, weak={weak}) source_len={} ops={} scalar_terminal={} par_stream={}",
            inner.source.len(),
            inner.ops.len(),
            inner.has_scalar_terminal,
            inner.par_stream,
        );
        return;
    }

    // Blessed object — class + inner ref.
    if let Some(b) = v.as_blessed_ref() {
        let ptr = Arc::as_ptr(&b) as usize;
        let strong = Arc::strong_count(&b);
        let weak = Arc::weak_count(&b);
        let _ = writeln!(
            out,
            "{prefix}BLESSED {} @ 0x{ptr:x} (strong={strong}, weak={weak})",
            b.class,
        );
        return;
    }

    // Async task — type + ptr only (no public state accessors).
    if let Some(t) = v.as_async_task() {
        let ptr = Arc::as_ptr(&t) as usize;
        let strong = Arc::strong_count(&t);
        let weak = Arc::weak_count(&t);
        let _ = writeln!(
            out,
            "{prefix}AsyncTask @ 0x{ptr:x} (strong={strong}, weak={weak})",
        );
        return;
    }

    // Atomic cell — show inner value.
    if let Some(arc) = v.as_atomic_arc() {
        let ptr = Arc::as_ptr(&arc) as usize;
        if !visited.insert(ptr) {
            let _ = writeln!(out, "{prefix}ATOMIC @ 0x{ptr:x} ...cycle");
            return;
        }
        let strong = Arc::strong_count(&arc);
        let weak = Arc::weak_count(&arc);
        let inner = arc.lock().clone();
        let _ = writeln!(
            out,
            "{prefix}ATOMIC @ 0x{ptr:x} (strong={strong}, weak={weak}) →",
        );
        dump_into(out, &inner, depth + 1, visited);
        visited.remove(&ptr);
        return;
    }

    // Fallback: name + best-effort length-via-int and stringification preview.
    // Catches everything else (sketches, channels, deques, kvstores, etc.) at
    // a uniform low resolution.
    let s = v.to_string();
    let _ = writeln!(
        out,
        "{prefix}{tag} {:?}",
        short_string(&s, 80),
    );
}

fn hex_preview(b: &[u8], cap: usize) -> String {
    let n = b.len().min(cap);
    let mut s = String::with_capacity(n * 3 + 8);
    for (i, byte) in b.iter().take(n).enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{:02x}", byte);
    }
    if b.len() > cap {
        let _ = write!(s, " …(+{} more)", b.len() - cap);
    }
    s
}

fn short_string(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let truncated: String = s.chars().take(cap).collect();
    format!("{}…(+{} chars)", truncated, s.chars().count() - cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn god_dump_integer_is_immediate_one_liner() {
        let v = StrykeValue::integer(42);
        let s = god_dump(&v);
        assert!(s.contains("INTEGER 42 (immediate)"), "got: {s}");
        assert_eq!(s.lines().count(), 1);
    }

    #[test]
    fn god_dump_bytes_shows_hex_preview_and_len() {
        let v = StrykeValue::bytes(Arc::new(vec![0x00, 0xff, 0x7f, 0x80, 0x01]));
        let s = god_dump(&v);
        assert!(s.contains("BYTES @ 0x"), "got: {s}");
        assert!(s.contains("len=5"), "got: {s}");
        assert!(s.contains("hex=00 ff 7f 80 01"), "got: {s}");
    }

    #[test]
    fn god_dump_undef_one_liner() {
        let s = god_dump(&StrykeValue::UNDEF);
        assert_eq!(s.trim(), "undef");
    }
}
