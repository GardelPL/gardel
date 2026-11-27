# Gardel parsing (`gparse`)

A cursor over bytes for writing parsers (Task H.7). `gjson`, `ghttp` and `grest`'s URL layer each wrote their own reading loop, with their own offset arithmetic, and each proved the same facts again: that a read stays in the text, and that it moves on. `CURSOR` states them once.

```toml
[dependencies]
gparse = { path = "../gparse" }
```

## The cursor

A `CURSOR` is a text of bytes and the offset of the next byte to read, with invariant `offset <= input.length`. It is a value: reading gives a new cursor and leaves the old one where it was, so a parser can try one form and go back to try another.

| Routine | Gives |
| :--- | :--- |
| `CURSOR.start(b)` | a cursor at offset 0 of `b` |
| `input`, `offset`, `remaining`, `at_end` | where it is |
| `peek`, `sees(b)` | the next byte, without reading it |
| `advance(n)` | the cursor `n` bytes on, for `n <= remaining` |
| `since(from)` | the bytes read since an earlier cursor on the same text |
| `expect(word)`, `expect_byte(b)` | past `word` or `b`, if the text goes on with it |
| `skip_while(keep)`, `take_while(keep)` | past every byte `keep` keeps, and those bytes |
| `take_some(keep)` | as `take_while`, but at least one byte |
| `read_nat(limit)` | a decimal number of one digit or more, at most `limit` |
| `fail(what)` | a `PARSE_ERROR` at this offset |

The readers that can fail return a `RESULT[PARSE_ERROR, ...]`. A `PARSE_ERROR` has the `offset` where the reader stopped and what it `expected` there, a `PARSE_EXPECTED`: `MoreBytes`, `Word`, `KeptByte`, `Digit` or `SmallerNumber`. A client keeps its own error type and maps these onto it.

## What is proven

Each fact is stated on `input.model`, the bytes as a list, so a client proves what it read with `LIST_OPS.at` and no body of this package.

- **Progress or failure.** Every reader that can fail ensures that an `Ok` cursor is on the same text and past this one, and that an `Error` is at this one's offset. A loop of such readers ends on `variant c.remaining`, with no proof of its own: `tests/src/gparse_text.gdl`'s `fields` is one. `skip_while` and `take_while` may read nothing, so they ensure only that they do not go back, and a loop on them alone is refused.
- **What was read.** `expect` read `word`, byte for byte. `take_while` kept every byte it passed and stopped at one it does not keep, or at the end. `read_nat` read only digits, stopped at a non-digit or the end, and gives `DECIMAL.value` of them, at most `limit`.
- **Why it failed.** `MoreBytes` exactly at the end of the text; `Digit` or `KeptByte` exactly when the next byte is not one; `SmallerNumber` only from `read_nat`, at the number's first digit. A limit is checked before each digit is added, so no read overflows.

`tests/` has the cases, which `gardelc test gparse` runs. `gardelc/tests/gparse.rs` also has a client that proves the facts above through the contracts alone, and the claims past them that are refused.

## Not yet

The plan (H.7) moves `grest`'s URL layer onto the cursor first, then `ghttp`'s head parser, then `gjson` if its round-trip proof survives the move, measuring each. A token reader is `take_some` with the format's own byte test. A quoted-string reader is left until a client needs it: JSON's, HTTP's and TOML's escapes differ.
