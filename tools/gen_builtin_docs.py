#!/usr/bin/env python3
"""Generate /// doc comments for undocumented fn builtin_* functions.

Reads src/builtins.rs, analyzes each function's name + body to infer a
one-line description, inserts /// above the fn if missing, writes back.

Run once then commit — the /// comments become permanent source.

    python3 tools/gen_builtin_docs.py
"""
import re
import sys

def fn_name_to_human(name: str) -> str:
    """builtin_c_to_f → 'c to f', builtin_is_prime → 'is prime'."""
    s = name.removeprefix("builtin_")
    return s.replace("_", " ")

def infer_doc(fn_name: str, body: str, sig: str) -> str:
    """Generate a doc string from function name + body patterns."""
    human = fn_name_to_human(fn_name)
    words = human.split()

    # Detect return type from body
    returns_bool = "bool_iv(" in body
    returns_float = "PerlValue::float(" in body
    returns_int = "PerlValue::integer(" in body and not returns_bool
    returns_string = "PerlValue::string(" in body
    returns_array = "PerlValue::array(" in body or "PerlValue::array_ref(" in body
    returns_undef_on_fail = "PerlValue::UNDEF" in body

    # Detect topic default
    uses_topic = "first_arg_or_topic" in body
    uses_interp = "interp:" in sig

    # Pattern: unit conversion (unit_scale)
    if "unit_scale" in body:
        # Extract the lambda
        m = re.search(r'unit_scale\(interp,\s*args,\s*\|(\w+)\|\s*(.+?)\)', body)
        if m:
            var, expr = m.group(1), m.group(2).strip()
            return f"Unit conversion: `{human}`. Computes `{expr}` from the input.{' Defaults to `$_`.' if uses_topic else ''}"
        return f"Unit conversion: `{human}`.{' Defaults to `$_`.' if uses_topic else ''}"

    # Pattern: is_* predicate
    if words[0] == "is" and returns_bool:
        what = " ".join(words[1:])
        return f"Test whether the argument is {what}. Returns 1 (true) or 0 (false).{' Defaults to `$_`.' if uses_topic else ''}"

    # Pattern: *_to_* conversion
    if "_to_" in human:
        parts = human.split(" to ")
        if len(parts) == 2:
            return f"Convert {parts[0]} to {parts[1]}.{' Defaults to `$_`.' if uses_topic else ''}"

    # Pattern: from_* parsing
    if words[0] == "from":
        what = " ".join(words[1:])
        return f"Parse a {what} string and return the numeric value. Returns `undef` on invalid input."

    # Pattern: to_* formatting
    if words[0] == "to" and not "_to_" in human:
        what = " ".join(words[1:])
        return f"Format the input as {what}.{' Defaults to `$_`.' if uses_topic else ''}"

    # Pattern: has_* predicate
    if words[0] == "has" and returns_bool:
        what = " ".join(words[1:])
        return f"Test whether the argument has {what}. Returns 1 or 0."

    # Pattern: count_*
    if words[0] == "count":
        what = " ".join(words[1:])
        return f"Count {what} in the input.{' Defaults to `$_`.' if uses_topic else ''}"

    # Pattern: extract_*
    if words[0] == "extract":
        what = " ".join(words[1:])
        return f"Extract all {what} from the input string. Returns a list."

    # Pattern: format_*
    if words[0] == "format":
        what = " ".join(words[1:])
        return f"Format the input as a human-readable {what} string.{' Defaults to `$_`.' if uses_topic else ''}"

    # Pattern: random_*
    if words[0] == "random":
        what = " ".join(words[1:])
        return f"Generate a random {what}."

    # Fallback based on return type
    type_hint = ""
    if returns_bool:
        type_hint = " Returns 1 (true) or 0 (false)."
    elif returns_float:
        type_hint = " Returns a float."
    elif returns_int:
        type_hint = " Returns an integer."
    elif returns_string:
        type_hint = " Returns a string."
    elif returns_array:
        type_hint = " Returns a list."

    topic_hint = " Defaults to `$_` when called with no args." if uses_topic else ""

    # Capitalize first letter
    desc = human[0].upper() + human[1:] if human else "Builtin function"
    return f"{desc}.{type_hint}{topic_hint}"

def process(path: str) -> int:
    with open(path, "r") as f:
        lines = f.readlines()

    added = 0
    out = []
    i = 0
    while i < len(lines):
        line = lines[i]

        # Check if this is a fn builtin_* line without a preceding ///
        m = re.match(r'^(pub\s+)?fn (builtin_\w+)\(', line.strip())
        if m:
            fn_name = m.group(2)
            # Check if already has /// above
            has_doc = False
            j = len(out) - 1
            while j >= 0 and out[j].strip() == "":
                j -= 1
            if j >= 0 and out[j].strip().startswith("///"):
                has_doc = True

            if not has_doc:
                # Read the function body to analyze
                body_lines = []
                brace_depth = 0
                started = False
                for k in range(i, min(i + 50, len(lines))):
                    body_lines.append(lines[k])
                    brace_depth += lines[k].count("{") - lines[k].count("}")
                    if "{" in lines[k]:
                        started = True
                    if started and brace_depth <= 0:
                        break
                body = "".join(body_lines)

                doc = infer_doc(fn_name, body, line)
                # Get the dispatch name (strip builtin_ prefix)
                dispatch_name = fn_name.removeprefix("builtin_")
                doc_line = f"/// `{dispatch_name}` — {doc}\n"
                out.append(doc_line)
                added += 1

        out.append(line)
        i += 1

    with open(path, "w") as f:
        f.writelines(out)

    return added

if __name__ == "__main__":
    path = sys.argv[1] if len(sys.argv) > 1 else "src/builtins.rs"
    n = process(path)
    print(f"added {n} /// doc comments to {path}")
