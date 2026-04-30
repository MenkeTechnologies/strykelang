# Help

## Running the tests

Each exercise ships with a stryke test file in `t/`. From the exercise directory, run:

```bash
stryke test
```

`stryke test` auto-discovers `t/` (or `tests/`) and runs every `*.stk` test file inside.
To run a single test file directly:

```bash
stryke t/test_allergies.stk
```

To run every exercism exercise at once:

```bash
for d in examples/exercism/*/; do (cd "$d" && stryke test); done
```

Test assertions in stryke use `assert_eq`, `assert_ok`, and friends — see the
[stryke testing docs](https://github.com/MenkeTechnologies/strykelang) for the full list.

## Submitting your solution

Submit using:

```bash
exercism submit Allergies.stk
```

Submitting an incomplete solution is fine — it lets you see how others have completed
the exercise and request mentor help.

## Need help?

- The [Stryke track's documentation](https://exercism.org/docs/tracks/stryke)
- The [Stryke language repo](https://github.com/MenkeTechnologies/strykelang)
- [Exercism's programming category on the forum](https://forum.exercism.org/c/programming/5)
- The [Frequently Asked Questions](https://exercism.org/docs/using/faqs)
