# Top-N CPAN harness (Phase 4)

Runs **`stryke`** against a curated set of **pure-Perl** modules installed under `local/lib/perl5`
(via `cpanm`), plus stubs under `vendor/perl/`. This is **not** full upstream test suites; it is a
**smoke** that `require` works and a few API calls match expectations.

## One-time setup (developers / CI)

From the repo root:

```sh
bash parity/cpan_topn/install_deps.sh
```

Requires **`cpanm`** (`cpanminus` package on Debian/Ubuntu, or `curl -L https://cpanmin.us | perl - --sudo App::cpanminus`).

Installs into **`parity/cpan_topn/local/`** (gitignored). Re-run after changing `MODULES.txt`.

## Run

```sh
cargo build --release --locked
bash parity/cpan_topn/run_cpan_topn.sh
```

Env: **`STRYKE`** (default `target/release/stryke`). **`stryke` does not read `PERL5LIB`**; the harness passes **`-I vendor/perl -I parity/cpan_topn/local/lib/perl5`** — `vendor/perl` is listed first so its in-tree stubs (e.g. `Carp.pm`) shadow the installed CPAN trees.

## Running it

This harness is run manually (it is not wired into a GitHub Actions job): run `install_deps.sh` once, then `run_cpan_topn.sh`.

The module list lives in **`MODULES.txt`** and is summarized in **`PARITY_ROADMAP.md`** Phase 4.
