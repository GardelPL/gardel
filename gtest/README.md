# Gardel tests (`gtest`)

What a package's test programs are written with (Task H.9). A package's tests are a package in its `tests/` directory that depends on it and on `gtest`. Each `program` there is a test, and `gardelc test` runs them:

```
gardelc test [DIR] [--filter NAME]
```

It verifies `DIR/tests` under `--strict` and builds it, once, runs each program in `DIR/tests` with `tests/cases.txt` on its standard input when there is one, and reports each by its exit status. It exits with 0 when every program passed, 1 when one failed or the tests did not verify or build, and 2 for a wrong command line. A program that fails has its output shown, and one that runs for ten minutes is stopped.

```toml
[dependencies]
mypackage = { path = ".." }
gtest = { path = "../../gtest" }
```

## Checks and the runner

Each check gives a `TEST_RESULT`, `Pass(name)` or `Fail(name, message)`:

| Check | Fails with |
| :--- | :--- |
| `CHECK.that(name, b)` | `is false` |
| `CHECK.equal_text(name, got, expected)` | `expected "abc", got "ab"` |
| `CHECK.equal_nat`, `CHECK.equal_int` | `expected 4, got 3` |
| `CHECK.equal_shown(name, got, expected, same, show)` | both values, each by `show` |
| `CHECK.is_ok`, `is_error`, `is_some`, `is_none` | `expected Ok, got an Error`, and the like |

Only a `program` does IO, so a test's `main` prints the report and exits with its code:

```eiffel
main(args: SEQ[STRING]): IO[EXIT_CODE]
  do
    let rs = LIST.empty
      .prepend(CHECK.equal_nat("twice", DOUBLE.twice(21), 42))
      .prepend(CHECK.is_error("not a number", NUMBER_TEXT.parse_nat("x")))
    with IO
      IO.print_line(TESTS.report(rs))
      IO.exit(TESTS.exit_code(rs))
    end
  end
```

`TESTS.report` lists each failure, `FAIL name: message`, and ends with `n checks: p passed, f failed`. `TESTS.exit_code` is 0 when every check passed, else 1.

## Cases files

A cases file has one case a line: an input, ` => `, and the text it must give. Empty lines and lines that start with `#` are skipped. `%XX` stands for the byte with hex value `XX`, on either side.

- `CASES.read(line)` is `Blank`, `Pair(input, expected)` or `NoArrow`.
- `CASES.check(rs, n, line, run)` puts the result of line `n` before `rs`: `run(input)` against the expected text, named `line n: input`. A blank line adds nothing, and a line with no ` => ` fails.
- `CASES.text(s)` and `CASES.bytes(s)` read the `%XX` escapes, as text or as bytes.

The program reads its lines itself. `tests/src/gtest_cases.gdl` and `gjson/tests/src/gjson_test.gdl` show the loop: about twenty lines, which reverse the results at the end so they print in order.

## Property tests

`PROPERTY.for_all(name, gen, n, seed, prop)` checks `prop` on `n` values that `gen` makes from `seed`. A failure says `case i of n fails, from seed s`: a run from seed `s` makes that value first. `PROPERTY.for_all_shown` adds the value, written by a `show` function.

The values come from SplitMix64 (`SPLIT_MIX`), with wrapping arithmetic: pure, and the same on every system. Its outputs from seed 0 are checked against the reference implementation's.

A `GEN[A]` is a step from a state to a value and the next state. `GENS` has:

- `u64`, `nat_below(n)`, `nat_in(lo, hi)` and `bool`;
- `list(g, max_length)`;
- `text(max_length)`, printable ASCII, and `text_from(alphabet, max_length)`;
- `map(g, f)` and `pair(g, h)`.

`GEN.make(step)` takes any other step.

## Reactive steps

`SCRIPT.play(step, state, events)` runs a reactive `step` over a list of events. The result is `Running(state, commands)`, or `Stopped(code, commands)` at the first `Stop`, with every command in order. `SCRIPT.printed(commands)` is the text of each `Print` and `Write`. The `step` must be a routine of a module, since nothing can name a `reactive program`.

## Known limits

- The runner's report is text up to about a megabyte; past that, the rest is left out.
- A routine that returns a `SCRIPTED[NAT]`, or another `SCRIPTED` of a machine number, must show its state in range, and `SCRIPT.play` is generic, so its result has no range. Keep the run in a `let`.
- Inside a lambda, `=` on a value whose type the checker cannot infer is refused (E0311), as `LIST_OPS.reverse(xs) = xs`. Use a named routine.
- A tuple on the line after a `let` is read as a call of the `let`'s value: `let r = f(s)` then `(a, b)` on the next line fails to parse. Bind the tuple, `let out = (a, b)`, or build it with a routine, as `GENS.both`.
