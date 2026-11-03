# Smart strings in Gardel: representation, Unicode, and contracts

**Yes, a smart string implementation makes sense for Gardel, but I would separate the logical string model from its physical representation.** That separation is particularly valuable in Gardel because contracts are proof obligations, invariants are part of types, and contracts are erased at runtime. gardel_language_specification_d…

The goal should be to make strings memory-efficient without forcing callers to reason about the underlying encoding.

Your concern about contracts identifies the central design problem: **an operation's correctness should not depend on which physical representation happens to store the string.**

There is also an important terminology distinction:

- **ASCII** is a character encoding for a limited character repertoire.
- **UTF-8** is a variable-width encoding of Unicode scalar values.
- **Unicode** defines the character repertoire and associated semantics; it is not, by itself, a physical encoding.
- **UTF-16** and **UTF-32** are additional encoding options, although I would not necessarily implement all of them in Gardel's initial standard library.

For example, a string containing only ASCII characters can be stored using one byte per character in UTF-8. There is no need for a separate ASCII storage format unless it provides a measurable advantage.

My recommendation is to make **UTF-8 the default representation and introduce compact ASCII storage only if benchmarks justify it**. If the compiler or runtime can represent ASCII strings with exactly the same storage as UTF-8 strings, an additional representation might not be worthwhile.

Let's examine the design from Gardel's perspective.

## 1. Separate three concepts that are easy to conflate

I would define three distinct layers.

| Layer | Responsibility | Example |
|---|---|---|
| `STRING` | The logical text value | `"Hello 世界"` |
| `STRING_REPR` | The physical encoding and storage | ASCII-compatible UTF-8 bytes |
| `CHAR` | One Unicode scalar value | `'世'` |

The public `STRING` type should expose text operations, not its storage encoding.

For example, a caller should be able to write:

```eiffel
let text = STRING.from_utf8(bytes)
let n = text.character_count
let c = text.character_at(0)
```

The public contract for `character_at` should not change depending on whether the string contains ASCII, accented characters, or characters outside the Basic Multilingual Plane.

The representation can change internally, but the observable behavior must remain consistent.

### An important distinction: code points versus characters

Gardel's specification already defines `CHAR` as a Unicode scalar value represented by `uint32_t`. gardel_language_specification_d…

That is a useful foundation. However, a Unicode scalar value is not necessarily what a user perceives as one character.

Consider these strings:

- `"é"` represented by U+00E9.
- `"é"` represented by U+0065 followed by U+0301 (the letter `e` and a combining acute accent).
- `"👨‍👩‍👧‍👦"` represented by several Unicode scalar values joined into one displayed emoji sequence.

These have different numbers of Unicode scalar values, UTF-8 bytes, and user-perceived grapheme clusters.

I would make the distinction explicit in the API:

- `byte_count`: number of encoded bytes.
- `code_point_count`: number of Unicode scalar values.
- `grapheme_count`: number of user-perceived grapheme clusters, if implemented.
- `is_ascii`: whether every code point is ASCII.

I would initially implement the first three only as far as the library can provide precise, verified semantics. In particular, grapheme segmentation requires Unicode rules and versioning; it should not be confused with counting scalar values.

## 2. What I would change in the current `STRING`

I inspected the current definition in `stdlib.gdl`. It has this representation:

```eiffel
module STRING

representation {NONE}
  items: LIST[U64]
  size: NAT
  limit: NAT
```

It also exposes `byte_length`, `capacity`, and `push_utf8_word`, with an invariant requiring positive capacity, consistency between `size` and the list length, and `size <= limit`.

This is a reasonable starting point for experimenting with storage, but it does not yet define a complete Unicode string abstraction.

In particular, `push_utf8_word` accepts a `U64` and increments the length by one. Its contract does not establish that the value represents a valid UTF-8 byte, that the sequence remains valid UTF-8, or that the operation preserves a Unicode string's logical meaning.

I would address this before introducing adaptive representations.

### The central problem with the current design

Your current invariant is essentially:

\[
I(s) \equiv
s.limit > 0
\land s.size = \operatorname{length}(s.items)
\land s.size \le s.limit
\]

This describes a bounded sequence of values. It does not establish that the sequence represents valid text.

For a Unicode string, you need an additional semantic property:

\[
\operatorname{decodeUTF8}(s.items) = s.model
\]

Here, `model` is a mathematical sequence of Unicode scalar values. The equation is conceptual: the actual implementation will need a specification that Gardel's verifier can represent and reason about.

This leads to an important design choice: **should every `STRING` be guaranteed to contain valid Unicode text, or should it also represent arbitrary bytes?**

I recommend keeping these concepts separate.

- `STRING` represents valid Unicode text.
- `BYTES` represents arbitrary binary data.
- A UTF-8 decoding operation converts `BYTES` into `OPTION[STRING]` or an appropriate `RESULT[STRING, ERROR]`.

That prevents network payloads, file contents, and other arbitrary byte sequences from accidentally being treated as valid text.

---

## 3. Two possible approaches to adaptive storage

There are two reasonable designs, but I would not give them equal priority.

### Option A: UTF-8 everywhere, with ASCII-aware optimizations

Store every string as UTF-8.

ASCII characters occupy one byte, while other Unicode scalar values occupy two to four bytes.

The public API stays independent of the storage details.

For example:

```eiffel
module STRING

representation {NONE}
  bytes: SEQ[U64]
  byte_count: NAT
  code_point_count: NAT
```

This is a conceptual sketch, not a complete implementation. In particular, the invariant would have to prove that the stored values are valid bytes, that the sequence is valid UTF-8, and that both counts are consistent with the encoded text.

**Advantages:**

- One encoding and one set of encoding invariants.
- Compact ASCII storage without a special ASCII variant.
- Straightforward interoperability with HTTP, JSON, and other UTF-8-based interfaces.
- No conversion is necessary when a string is passed to an API expecting UTF-8.
- A relatively small proof surface.

**Disadvantages:**

- Finding the character at an arbitrary code-point index requires decoding from an appropriate starting position, unless additional indexing information is maintained.
- Character counts and byte counts differ.
- UTF-8 validation and manipulation still require careful contracts.

This is my preferred initial design.

### Option B: A genuinely adaptive representation

If you want the representation to change according to the content, use a sum type.

For example:

```eiffel
module STRING

representation {NONE}
  | Ascii(bytes: SEQ[U64])
  | Utf8(bytes: SEQ[U64])
```

The idea is that `Ascii` is used when every byte is in the range 0–127, while `Utf8` is used for text requiring multibyte encoding.

This representation illustrates the architecture, but I would not adopt it unchanged. It duplicates the storage field, and it does not itself guarantee that `Utf8` contains valid UTF-8.

A more complete implementation would need representation-specific invariants:

\[
I_{\text{ASCII}}(b)
\equiv
\forall i < |b|,\; b_i \le 127
\]

\[
I_{\text{UTF8}}(b)
\equiv
\operatorname{ValidUTF8}(b)
\]

The overall invariant would then be:

\[
I(s) \equiv
\begin{cases}
I_{\text{ASCII}}(b) & \text{if }s=\operatorname{Ascii}(b)\\
I_{\text{UTF8}}(b) & \text{if }s=\operatorname{Utf8}(b)
\end{cases}
\]

Because valid ASCII is also valid UTF-8, these variants overlap semantically. That is not inherently incorrect, but it introduces a canonicalization question: should an ASCII-only value always use `Ascii`?

If the answer is yes, the UTF-8 variant also needs a constraint:

\[
I_{\text{UTF8}}(b)
\equiv
\operatorname{ValidUTF8}(b)
\land
\neg\operatorname{IsASCII}(b)
\]

Now the variants are disjoint, and the verifier can reason about a canonical representation.

However, this extra invariant is useful only if the representation actually saves memory or improves performance. If both variants use the same `SEQ[U64]` structure, the tag and duplicated representation may cost more than they save.

**My recommendation:** start with Option A. Introduce an adaptive representation only after benchmarking the actual backend and storage costs.

---

## 4. How to design contracts without encoding-dependent behavior

This is the most important part of the design.

Consider three operations:

1. `byte_length`
2. `code_point_count`
3. `character_at`

They should have contracts defined in terms of the logical text, not its representation.

Let \(M(s)\) denote the sequence of Unicode scalar values represented by string \(s\).

Define:

\[
\operatorname{CodePointCount}(s) = |M(s)|
\]

\[
\operatorname{ByteLength}(s) =
|\operatorname{UTF8Encode}(M(s))|
\]

Then the contracts become representation-independent.

| Operation | Preconditions | Postconditions |
|---|---|---|
| `byte_length` | None | Result equals the number of UTF-8 bytes |
| `code_point_count` | None | Result equals the number of Unicode scalar values |
| `character_at(i)` | \(i < \operatorname{CodePointCount}(s)\) | Result equals \(M(s)[i]\) |
| `is_ascii` | None | True exactly when every code point is ASCII |
| `concat(other)` | None | Result's logical model is the concatenation of both input models |
| `equals(other)` | None | True exactly when both logical text values are equal, according to the chosen equality semantics |

These contracts should hold regardless of whether the implementation stores ASCII separately from general UTF-8.

### Example: `character_at`

Suppose the string is `"AéB"`.

Its UTF-8 bytes are:

```text
41 C3 A9 42
```

Its byte length is 4, but its code-point count is 3.

Therefore:

```text
character_at(0) = 'A'
character_at(1) = 'é'
character_at(2) = 'B'
```

The contract should use the code-point index, not the byte offset.

A byte-oriented implementation may need to decode the prefix to find the correct location. An ASCII-specific implementation may use direct indexing. Both implementations must satisfy the same postcondition.

That is precisely what contracts are good at: **they let you change the implementation without changing the public specification.**

### Example: concatenation

Suppose the left string is ASCII and the right string contains non-ASCII characters.

The resulting string might be represented using the general UTF-8 variant, but callers should not need to know that.

The essential postcondition is:

\[
M(a \mathbin{+\!\!+} b) = M(a) \mathbin{+\!\!+} M(b)
\]

Here, \(+\!\!+\) denotes sequence concatenation.

The contract does not say anything about the output's physical representation. That is intentional.

---

## 5. Avoid a single invariant that becomes too complicated

Gardel's specification treats module invariants as properties that must hold for every value of the type. Constructors and transformations must establish those invariants. gardel_language_specification_d…

This is a powerful mechanism, but it means the representation design directly affects proof complexity.

For an adaptive string, I would separate the obligations into three levels.

### Level 1: Representation invariants

These describe the physical storage:

- The stored bytes are within the byte range.
- The stored length agrees with the storage structure.
- Capacity constraints hold, if capacity is part of the chosen representation.
- The encoding variant agrees with the bytes stored.

### Level 2: Encoding invariants

These describe the validity of the text:

- The bytes form valid UTF-8.
- Decoding produces Unicode scalar values.
- The decoded sequence agrees with the logical model.
- If the representation is ASCII-specific, all its bytes are ASCII.

### Level 3: Operation contracts

These describe the behavior users care about:

- Character indexing returns the expected scalar value.
- Concatenation preserves order.
- Equality compares logical text rather than implementation details.
- Length queries return the correct units.
- Conversions succeed only when their stated conditions hold.

This hierarchy is important because it prevents every public operation from having to restate all the encoding rules.

A verified constructor establishes the string invariant once. Other operations can rely on it.

### One caveat about the proof model

Your specification proposes a ghost model for collections such as `SEQ[A]`, with a corresponding logical sequence. gardel_language_specification_d…

The same approach is appropriate for strings, but there is an implementation question to resolve: can the current Gardel verifier express and prove the necessary UTF-8 decoding and encoding properties?

For example, a contract that refers to `ValidUTF8(bytes)` is not automatically useful merely because it is mathematically well-defined. The verifier needs a suitable specification and enough lemmas to establish that operations preserve it.

I would avoid starting with a single enormous UTF-8 validity predicate that every routine must prove from scratch.

Instead, build up the verification infrastructure with smaller properties, such as:

- Validity of a single encoded scalar value.
- Validity of concatenating two valid UTF-8 sequences.
- Preservation of validity when appending a complete encoded scalar value.
- Agreement between the logical model and the encoded representation.

These are suggested proof obligations, not capabilities I have verified against the current compiler implementation.

---

## 6. A better API boundary: distinguish text from encoding

I would design the public API around logical strings and explicit conversions.

| Routine | Purpose |
|---|---|
| `from_utf8(bytes)` | Decode UTF-8 bytes into valid text, returning failure for malformed input |
| `to_utf8` | Obtain the UTF-8 encoding of a string |
| `from_code_points(chars)` | Construct a string from Unicode scalar values |
| `byte_length` | Return UTF-8 byte length |
| `code_point_count` | Return Unicode scalar count |
| `character_at(index)` | Access a scalar by code-point index |
| `is_ascii` | Test whether the text consists entirely of ASCII |
| `concat(other)` | Concatenate two strings |
| `equals(other)` | Compare strings according to the defined equality semantics |
| `is_empty` | Test whether the logical string is empty |

I would also consider a separate `STRING_VIEW` later, especially if the runtime can safely support views without copying.

Two design decisions need particular care.

**First, equality.** Should equality mean exact equality of Unicode scalar sequences, or canonical equivalence? Under exact scalar-sequence equality, precomposed `"é"` and decomposed `"e\u0301"` are different strings. Under canonical equivalence, they may compare equal after normalization. I recommend exact scalar-sequence equality for the initial implementation, with normalization provided separately.

**Second, normalization.** UTF-8 validity does not imply Unicode normalization. If Gardel later supports NFC or NFD normalization, that should be a separate operation with its own contracts and Unicode-version policy.

---

## 7. The most important trade-off: memory versus proof complexity

An adaptive string representation can save memory, but the benefit depends on the runtime layout.

For example, storing ASCII in a byte buffer can use one byte per ASCII character. A general UTF-8 buffer already has that property, so a separate ASCII variant may not save anything.

By contrast, storing every character as a `U64` consumes eight bytes per code point before accounting for sequence overhead. Your current `LIST[U64]` representation is therefore not a compact UTF-8 byte representation.

I would investigate these three designs in order:

| Design | Memory efficiency | Verification complexity | Recommendation |
|---|---|---|---|
| `SEQ[U64]` storing UTF-8 bytes | Better than one `U64` per code point, but not necessarily one byte per byte | Moderate | Good initial prototype |
| Dedicated packed byte buffer | Potentially close to one byte per UTF-8 byte | Moderate; depends on the buffer model | Preferred production target |
| Adaptive ASCII/UTF-8 storage | Potentially useful, but benefits depend on layout and workload | Highest of these three | Benchmark before adopting |

The distinction between a `U64` containing one byte and an actual packed byte buffer is crucial. The former is logically byte-oriented but may still have substantial physical overhead.

For Gardel, I would make the choice based on measured runtime behavior, not on the assumption that more specialized representations are automatically more efficient.

---

## 8. My proposed design for Gardel

I would adopt the following decisions.

1. **`STRING` is always valid Unicode text.** Its logical model is a sequence of Unicode scalar values.
2. **UTF-8 is the canonical external encoding.** It is the format used at byte-oriented boundaries.
3. **`BYTES` is a separate type.** Arbitrary bytes do not automatically constitute valid text.
4. **The initial implementation uses UTF-8 internally.** Avoid multiple storage variants until benchmarks justify them.
5. **Contracts describe logical semantics.** Public behavior does not change with the storage representation.
6. **Length units are explicit.** Distinguish byte length from code-point count, and add grapheme count only with a precise specification.
7. **Normalization is separate.** Exact scalar-sequence equality is the initial default.
8. **Constructors establish validity.** Untrusted input must be decoded or validated before becoming a `STRING`.
9. **Encoding proofs are modular.** Prove reusable properties about UTF-8 encoding and decoding rather than duplicating those obligations across the API.

### The key architectural principle

Gardel should not expose a different string contract for ASCII and Unicode. It should expose **one logical string abstraction with one set of public contracts and, if necessary, several verified implementations**.

That fits Gardel's approach to abstract data types, invariants, and verification boundaries. It also leaves room for future optimizations without forcing every client to change when the representation changes.

The next concrete step is to revise the current `STRING` definition around a ghost sequence of `CHAR` values and a UTF-8 byte representation. Once the logical model and its invariants are clear, we can evaluate whether a dedicated ASCII variant provides enough benefit to justify the additional proof obligations.