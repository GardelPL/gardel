# Gardel JSON (`gjson`)

A verified JSON library (RFC 8259) written in Gardel: a value type, a parser and a writer.

```eiffel
match JSON.parse_text("{\"b\": [1, 2.5, \"x\"], \"a\": null}")
  case Ok(v) then v.to_text            -- {"a":null,"b":[1,2.5,"x"]}
  case Error(e) then NUMBER_TEXT.nat_to_text(e.offset)   -- the byte where it went wrong
end
```

## The value

`JSON` is opaque. It is built with `null`, `bool`, `int`, `number`, `string`, `empty_array`, `empty_object`, `with_item` and `with_field`. It is read with `is_null`, `is_array`, `is_object`, `as_bool`, `as_int`, `number_text`, `as_string`, `count`, `item`, `field` and `depth`.

Its invariant says the value is well-formed:

- Text is well-formed UTF-8.
- Object keys are unique: an object keeps its fields in ascending byte order of their keys.
- Nothing nests deeper than `JSON.max_depth` (64) levels.

**Numbers** (decision 2 of the plan): an integer in the range of `I64`, written without a fraction or an exponent, is an `I64`. Any other number keeps its literal text exactly as read, so no precision is lost and writing it gives the same text back. `-0` is not an integer here, so it keeps its sign.

Two equal values are written as the same bytes: the writer adds no white space and writes keys in order. A value is its cells and nothing else, so `=` is its equality (`CANONICAL_EQ`).

## Parsing

`JSON.parse(b: BYTES): RESULT[JSON_ERROR, JSON]` accepts one value with only white space around it. Every malformed input is an `Error`, never a crash, and each error names a byte offset:

| Error | When |
| :--- | :--- |
| `TooLarge` | More than `JSON.max_input` bytes (4 MiB) |
| `TooDeep` | Arrays and objects nested deeper than 64 levels |
| `UnexpectedEnd`, `UnexpectedByte` | The text stops, or a byte cannot start or continue a value. Outside strings only ASCII may appear. |
| `BadNumber` | A malformed number, such as `1.` or `01` |
| `ControlInString`, `BadEscape`, `LoneSurrogate`, `BadString` | A raw control character, an unknown escape, half of a surrogate pair, or text that is not well-formed UTF-8 |
| `DuplicateKey` | A key the object already has |
| `TrailingBytes` | Something other than white space after the value |

## What is proven

`gardelc --project gjson --strict` verifies with no `assume`:

- **The value is well-formed.** Every value the parser returns and every builder keeps the invariant: valid UTF-8, unique keys in order, depth at most 64.
- **The parser is total and bounded.** It terminates on every input, reads at most 4 MiB, and nests at most 64 levels.
- **The round trip, stage 1.** The law `JSON.scalar_round_trip` says `JSON.parse(v.to_bytes) = Ok(v)` for every scalar whose text fits `max_input`: `null`, booleans, integers (including `I64.min`), other numbers, and strings with every escape. A client proves it for its own values from the builders' contracts. For example, `JSON.parse(JSON.int(i).to_bytes) = Ok(JSON.int(i))` holds for every `i`.
- **Not yet proven: arrays and objects.** The round trip of an array or object is checked by tests only (below). The reader and writer of chains are proven to keep the invariant, but not to undo each other.

## Tests

`gjson/tests` is a package that depends on `gjson`, so it sees only contracts. Its cases are in `gjson/tests/cases.txt`, read on standard input. Each case is a document and what `parse` must say of it. Every accepted value must also read back as itself from what it writes, which covers the round trip of arrays and objects. Generated cases reach the limits: 64 and 65 levels of nesting, 200,000 items, 300,000 bytes of text, 50,000 escapes, 3,000 fields in reverse order, and a document over 4 MiB.

```
gardelc --project gjson/tests --strict
gjson/tests/build/gjson_test_bin.exe < gjson/tests/cases.txt
```

## Costs

- The parser reads the document as a list of bytes, about 32 bytes of memory per byte of input while it runs. The writer builds its text the same way.
- Arrays, objects, strings and numbers are read and written by loops, so a long array needs no stack. Only nesting uses the stack, at most 64 levels.
- Each field of an object read out of order is inserted into a sorted list. Reading an object whose keys arrive in reverse order is therefore quadratic in its number of fields: 3,000 fields take about a second. Keys in ascending order, as `gjson` writes them, cost one comparison each.
- `field`, `item`, `with_item` and `with_field` walk the chain, so each is linear in the size of the array or object.
