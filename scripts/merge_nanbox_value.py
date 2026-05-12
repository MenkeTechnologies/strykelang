#!/usr/bin/env python3
"""Splice nanboxed impl into src/value.rs between StructInstance and fn parse_number."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VALUE = ROOT / "src" / "value.rs"
BODY = ROOT / "scripts" / "nanbox_impl_body.rs"

def main() -> None:
    s = VALUE.read_text()
    i = s.find("impl StrykeValue {")
    j = s.find("\nfn parse_number")
    if i < 0 or j < 0:
        raise SystemExit(f"markers not found i={i} j={j}")
    body = BODY.read_text()
    if not body.endswith("\n"):
        body += "\n"
    out = s[:i] + body + s[j + 1 :]
    VALUE.write_text(out)
    print("merged", VALUE, "bytes", len(out))


if __name__ == "__main__":
    main()
