# Gardel REST (`grest`)

A verified REST layer for Gardel, built in Task F.6 of
[plan/libraries/verified_libraries.md](../plan/libraries/verified_libraries.md). It is in progress: this
version has the URL layer, which turns a request target into decoded path segments and query fields
and matches it against a path pattern. Requests, responses and the server come next, on `ghttp` and
`gjson`.

## Modules

| Module | What it does |
| :--- | :--- |
| `URL_TEXT` | Percent decoding (RFC 3986 section 2.1). In a query, `+` is also a space. A `%` with no two hex digits is `BadEscape`, and bytes that are not UTF-8 are `NotUtf8`. |
| `REQUEST_TARGET` | `/users/42?q=a+b` as segments `users`, `42` and the field `q` = `a b`. A segment is decoded after the split, so `%2F` stays inside it. Empty query fields are skipped, and a field with no `=` has an empty value. |
| `PATH_PATTERN`, `PATH_SEGMENT` | `/users/:id`: a segment that starts with `:` is a parameter, and the others must equal the request's decoded segment. |
| `URL_PARAM` | A decoded name and value, and `find`, the value of the first one with a name. |
| `URL_ERROR` | Why a target was refused, each one a `400`, with a short `reason`. |

A pattern is split as a target is: `/` is one empty segment, and `/users/` has two, `users` and an
empty one, so it does not match `/users`. Patterns are written decoded.

## What is proven

`gardelc --project grest --strict` proves:

- **Limits.** A parsed target has at most `REQUEST_TARGET.max_segments` (16) segments and
  `max_fields` (32) query fields. A target with more is refused, so the work of a match is bounded
  whatever a client sends.
- **A match gives one value per parameter.** If `pattern.match_target(t)` is `Some(ps)`, then `ps`
  has exactly `pattern.param_count` entries, never more than 16, and `t` has as many segments as the
  pattern.
- **Decoding never grows text,** and text with no `%` (nor `+`, in a query) decodes to itself,
  with no copy.
- **Only a valid pattern matches.** Any text gives a `PATH_PATTERN`, so a route can be written
  with no precondition to prove. A pattern that does not start with `/`, has more than 16 segments,
  holds a `?` or a `%`, or has a parameter with no name is not valid, and it matches nothing.

`gardelc/tests/grest.rs` checks that a client proves these through the contracts alone, and that a
claim past a limit is refused.

## Tests

`grest/tests` is a package that depends on `grest`. Its program reads the cases of
`grest/tests/cases.txt` on standard input. Each line is a decoding, a target, a pattern or a match,
and what it must give:

```text
gardelc --project grest/tests --strict
grest/tests/build/grest_test_bin.exe < grest/tests/cases.txt
```

Exit code 0 means every case holds. `cargo test --test grest` does both.
