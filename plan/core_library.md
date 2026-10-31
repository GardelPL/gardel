# Designing Gardel's Core Library

Given the current `SEQ`, `BIGNAT`, `STRING`, and `OPTION` modules, I would define Gardel's core library around one principle:

**The core library should provide a small set of composable, mathematically specified abstractions that programmers and AI agents can use to build programs with provable contracts.**

This is different from simply providing a large collection of convenience functions. In Gardel, a library operation should ideally have:
1. A clear semantic model.
2. Preconditions and postconditions where applicable.
3. Representation invariants for concrete data structures.
4. Algebraic laws that can be used to reason about client code.
5. A predictable implementation whose correctness can be verified.

Your existing modules provide a useful starting point, but they leave some important gaps. In particular, `SEQ` exposes a sequence representation and a `LIST[A]` model; `OPTION` represents optional values; `BIGNAT` represents non-negative numbers using limbs; and `STRING` represents UTF-8-related data using `U64` items. We should build on these abstractions rather than introduce a competing collection of types.

One important distinction: the code below your existing library is the **current baseline**. The proposed modules and operations are recommendations, not claims about features Gardel already supports.

## 1. The essential modules

I would organize the core library into three layers.

| Layer | Module | Purpose | Priority |
|---|---|---|---|
| **1. Foundations** | `OPTION[A]` | Represent a value that may be absent | Existing; extend |
| | `RESULT[E, A]` | Represent success or an explicit failure | **P0 — Add** |
| | `SEQ[A]` | Represent and manipulate sequences | Existing; extend |
| | `STRING` | Represent and manipulate text | Existing; clarify semantics |
| | `BIGNAT` | Represent arbitrary-precision natural numbers | Existing; fix semantics |
| **2. Functional combinators** | Sequence operations | `map`, `filter`, folds, composition, and queries | **P0 — Add** |
| | Option combinators | `map`, `and_then`, defaults, and fallback | **P0 — Add** |
| | Function composition | Compose functions and reuse transformations | **P1 — Add** |
| **3. Collections and algorithms** | `SET[A]` | Represent collections of distinct values | P2 |
| | `MAP[K, V]` | Associate keys with values | P2 |
| | `PAIR[A, B]` | Represent two related values, if not already built in | P2 |
| | Numeric algorithms | Addition, multiplication, comparison, division, etc. | P1 |

The priorities assume that the corresponding functionality is not already supplied by Gardel's language primitives or another existing module.

I would **not** introduce a separate module for every familiar functional-programming concept. In particular, folds, composition, and most collection transformations can initially be ordinary, reusable functions rather than new language constructs.

---

# 2. `OPTION[A]`: make it composable

Your current `OPTION[A]` provides two fundamental operations:

- `is_some`: determine whether a value exists.
- `get`: retrieve the value, with a precondition that it exists.

That is a reasonable starting point. However, `get` makes clients handle the possibility of absence themselves, and it is a partial operation: calling it on `None` violates its precondition.

The next step should be to make `OPTION` composable.

### Essential operations

| Operation | Meaning |
|---|---|
| `is_some` | Check whether a value exists |
| `is_none` | Check whether a value is absent |
| `get` | Retrieve a present value, subject to its precondition |
| `map(f)` | Transform the contained value, if present |
| `and_then(f)` | Apply a function that itself returns an option |
| `unwrap_or(default)` | Return the contained value or a default |
| `or_else(f)` | Supply an alternative option when the original is absent |

For example, conceptually:

```text
Some(10).map(double) = Some(20)

None.map(double) = None
```

And:

```text
Some(10).and_then(lookup_customer) = lookup_customer(10)

None.and_then(lookup_customer) = None
```

These examples are illustrative; they do not assume that Gardel supports this exact invocation syntax.

### The laws matter more than the number of features

For `map`, the essential algebraic laws are:

**Identity**

Applying the identity function does not change the option.

`map(identity) = identity`

**Composition**

Mapping two functions sequentially is equivalent to mapping their composition.

`map(f).map(g) = map(g ∘ f)`

**Absence preservation**

Mapping over `None` produces `None`.

These laws give the verifier useful properties to establish and give AI-generated code reusable reasoning rules.

**Recommendation:** Keep the existing representation and add `map`, `and_then`, and `unwrap_or` first. Add other convenience operations only when they provide real value.

---

# 3. `RESULT[E, A]`: distinguish absence from failure

This is the most important missing abstraction in the proposed baseline.

`OPTION[A]` answers:

> Is there a value?

It cannot, by itself, explain why an operation failed.

Consider parsing an integer from a string. The result might be:
- a successfully parsed number;
- an invalid character;
- an empty input;
- an out-of-range value.

Returning `None` for all failures loses the reason for failure.

A `RESULT[E, A]` can represent either an error of type `E` or a successful value of type `A`.

Conceptually:

```text
Error(error)
Success(value)
```

These constructor names are illustrative, not prescribed Gardel syntax.

### Essential operations

| Operation | Meaning |
|---|---|
| `is_success` | Determine whether the operation succeeded |
| `map(f)` | Transform a successful value |
| `and_then(f)` | Chain an operation that may fail |
| `map_error(f)` | Transform the error |
| `unwrap_or(default)` | Return the success value or a default |

The most important operation is `and_then`, because it allows failure-aware computations to be composed without repeatedly checking and unpacking results.

For example, a parser pipeline might conceptually be:

`parse_input → validate_input → convert_value`

Each stage can fail, and the first failure is propagated.

### Why this matters for Gardel

A function's contract can specify its valid inputs and the meaning of its successful result. When failure is part of the expected behavior, `RESULT` makes that behavior explicit in the return type.

This is especially valuable for:
- parsers and serializers;
- file and network protocols;
- validation pipelines;
- external-service integrations;
- AI-generated programs that must handle failure explicitly.

It also provides a clear distinction between **an expected failure represented in the type** and **a contract violation**.

A failed parse can be a valid result of a correctly executed function. Calling a function with an invalid argument can instead violate its precondition.

Those are different situations, and Gardel should make that distinction easy to express.

**Recommendation: P0.** Introduce `RESULT` before expanding the rest of the library.

---

# 4. `SEQ[A]`: establish the functional core

Your `SEQ[A]` is the natural center of Gardel's functional library.

It already has a useful structural representation:

```text
Empty
Leaf(value)
Concat(left, right)
```

Its model maps the representation to `LIST[A]`. That is a promising design because the public semantics can be specified independently of the concrete representation.

For example, `Concat(left, right)` can be implemented as a tree node while its abstract meaning remains the concatenation of the two sequences.

This separation should become a defining principle of the library.

## 4.1 Essential sequence operations

I would prioritize the following API.

| Operation | Purpose | Priority |
|---|---|---|
| `empty` | Construct an empty sequence | Existing |
| `singleton(x)` | Construct a one-element sequence | Existing |
| `concat(other)` | Concatenate two sequences | Existing |
| `length` | Obtain the number of elements | Existing; strengthen |
| `is_empty` | Test whether the sequence is empty | P0 |
| `map(f)` | Transform every element | P0 |
| `filter(p)` | Keep elements satisfying a predicate | P0 |
| `fold_left(f, initial)` | Reduce elements from left to right | P0 |
| `fold_right(f, initial)` | Reduce elements from right to left | P0 |
| `reverse` | Reverse element order | P1 |
| `contains(x)` | Test membership | P1; requires equality semantics |
| `all(p)` | Check whether every element satisfies a predicate | P1 |
| `any(p)` | Check whether at least one element satisfies a predicate | P1 |
| `take(n)` | Obtain a prefix | P1 |
| `drop(n)` | Remove a prefix | P1 |
| `zip(other)` | Pair corresponding elements | P2 |

I would defer sorting, grouping, set conversion, and advanced collection operations until the fundamental abstractions and their contracts are stable.

### What about `head` and `tail`?

They are useful, but the API should make empty-sequence behavior explicit.

Two reasonable designs are:

- Return an `OPTION` when the sequence might be empty.
- Require a non-empty sequence through a precondition or a distinct non-empty abstraction.

For a small functional core, I would prefer an option-returning operation when empty input is an ordinary possibility. A precondition is appropriate when non-emptiness is genuinely a caller obligation.

## 4.2 Algebraic laws for `SEQ`

The real value comes from specifying the behavior of these operations mathematically.

### Concatenation

The empty sequence is an identity:

- `empty.concat(s)` represents the same sequence as `s`.
- `s.concat(empty)` represents the same sequence as `s`.

Concatenation is associative:

`(a.concat(b)).concat(c)` represents the same sequence as `a.concat(b.concat(c))`.

The two expressions need not have the same internal tree structure. They must have the same abstract sequence.

This distinction is important for Gardel: **equivalence of models should not require equality of representations**.

### Mapping

Mapping the identity function changes nothing:

`map(identity)` is equivalent to the identity transformation.

Mapping composed functions is equivalent to mapping each function sequentially:

`map(g ∘ f) = map(g) ∘ map(f)`

This law is particularly useful for optimizing functional pipelines.

### Filtering

Filtering by a predicate preserves the relative order of the retained elements.

Filtering by a predicate that always evaluates to `True` produces the original sequence. Filtering by one that always evaluates to `False` produces the empty sequence.

### Length

The abstract length of a concatenation is the sum of the abstract lengths, provided that the numeric semantics permit that sum to be represented.

This last qualification matters for your implementation, as discussed below.

## 4.3 Folds should be library functions, not new syntax

I would initially implement `fold_left` and `fold_right` as ordinary library operations.

They support many useful functions:
- sum and product;
- element counts;
- conversion and serialization;
- aggregation;
- reconstruction of collections;
- interpreters and tree algorithms.

They also provide a common vocabulary for explaining algorithms to AI agents.

However, the library must define the order in which the combining function is applied. The order is observable when the function is not pure, and different folds are not generally interchangeable even when the combining function is pure.

Gardel's semantics should make the distinction explicit.

---

# 5. Fix the mathematical contracts of the existing modules first

Before implementing a larger API, I would review several issues in the existing definitions.

These are not merely missing convenience functions. They affect whether the library's specifications accurately describe its behavior.

## 5.1 `SEQ.length`: the overflow fallback needs a defined meaning

Your current implementation contains this branch:

```text
if ll <= 9223372036854775807 and rl <= 9223372036854775807 then
  ll + rl
else
  ll
end
```

The fallback returns `ll` when the condition fails.

That is not generally the length of the concatenated sequence.

For example, if the left sequence has length 10 and the right sequence has a length exceeding the stated bound, returning 10 does not satisfy the ordinary length semantics.

There are two separate questions here:

1. What is the numeric range of `NAT`?
2. What should `length` do if the mathematical length is outside that range?

The library needs a consistent answer.

Possible designs include:
- Make `NAT` arbitrary precision, so the mathematical sum is representable.
- Define a bounded sequence length and use an explicit precondition that guarantees the sum fits.
- Return a result type that represents overflow explicitly.

The right choice depends on Gardel's integer semantics. The current fallback should not silently substitute a different value.

The contract should establish a relationship such as:

`Result = Current.model.length`

subject to the language's actual numeric semantics.

## 5.2 `BIGNAT.model` does not currently model the represented number

This is the most important issue in `BIGNAT`.

Your current representation is:

```text
representation {NONE}
  limbs: LIST[U64]
  size: NAT
```

But the model is:

```text
model: NAT
  do
    Current.size
  end
```

Consequently, the model returns the **number of limbs**, not the numeric value represented by those limbs.

For example, a representation with one limb containing `42` has model value `1`, not `42`.

That makes the current model unsuitable for specifying arithmetic operations on arbitrary-precision natural numbers.

### What should change?

First, define the limb interpretation precisely.

For example, if the representation uses a fixed radix \(B\), and the limbs are stored least-significant first, the mathematical value is:

\[
\operatorname{value}(x)
=
\sum_{i=0}^{n-1} x_i B^i
\]

Here, \(x_i\) is the limb at position \(i\), and \(n\) is the number of limbs.

This is illustrative mathematics: Gardel's actual radix, limb order, and permitted limb values must be specified before adopting this model.

Then distinguish two concepts:

- `model`: the mathematical natural number represented by the object.
- `limb_count`: the number of storage limbs.

The latter is a representation property. It should not be confused with the abstract value.

I would also review these details:

- `from_u64(x)` currently requires `x > 0`, so it excludes zero even though `BIGNAT.zero` exists.
- The representation invariant should define whether leading zero limbs are permitted.
- `push_high_limb` needs a precise definition of how a new limb changes the mathematical value.
- The 1024-limb bound should be treated as a representation capacity constraint, not as part of the mathematical definition of natural numbers.

Once the model is correct, you can specify addition and multiplication against ordinary natural-number arithmetic.

**Recommendation:** Correct the model before adding more arithmetic operations.

## 5.3 `STRING`: distinguish bytes from characters

Your representation is:

```text
items: LIST[U64]
size: NAT
limit: NAT
```

The feature `push_utf8_word(w: U64)` adds one item and increments `size` by one.

That leaves an important semantic question unresolved: what does one item represent?

If each item represents a byte of UTF-8 data, then the representation needs to constrain each item to the byte range. It must also define whether the entire sequence is required to be valid UTF-8.

If each item represents a Unicode scalar value, the valid range and length semantics are different.

These are distinct abstractions:

| Abstraction | Element | Meaning of length |
|---|---|---|
| Byte sequence | Byte | Number of bytes |
| Unicode string | Unicode scalar value | Number of scalar values |
| UTF-8 encoded string | Byte sequence with encoding constraints | Number of encoded bytes, unless another length operation is explicitly provided |

For an initial core library, I would keep the public abstraction simple:

- Define exactly what `STRING` represents.
- Specify whether `byte_length` counts encoded bytes or elements.
- Add concatenation and equality.
- Add UTF-8 validation and conversion operations with explicit failure semantics.

If you need a general-purpose byte buffer independently of text, a distinct `BYTE_STRING` or `BYTE_SEQ` abstraction may be appropriate. I would not add it solely for naming purposes if `SEQ[U64]` can already provide the required behavior.

## 5.4 `SEQ` and `LIST`: define their relationship

`SEQ` uses `LIST[A]` as its model, and your implementation uses `LIST.empty`, `prepend`, `append`, and `length`.

This raises an architectural question: is `LIST` a built-in language abstraction, an existing library module, or an implementation-level type?

If `LIST` is a foundational abstraction, document its semantics and keep it separate from the public `SEQ` implementation.

I would use the following distinction:

- `LIST[A]`: the mathematical or foundational sequence model.
- `SEQ[A]`: the concrete sequence abstraction whose representation can be optimized independently.

This arrangement is useful only if the distinction is intentional. If `LIST` and `SEQ` are meant to be interchangeable public collections, the library should instead define a clear reason for maintaining both.

---

# 6. Function composition: a small feature with a large payoff

Function composition deserves a place in Gardel's core functional vocabulary.

Mathematically, for functions \(f : A \to B\) and \(g : B \to C\),

\[
(g \circ f)(x) = g(f(x))
\]

The result is a function from \(A\) to \(C\).

Composition is useful because it allows a transformation to be described as a sequence of smaller transformations. It also has simple algebraic laws.

### Identity

\[
f \circ id = f
\]

\[
id \circ f = f
\]

### Associativity

\[
h \circ (g \circ f) = (h \circ g) \circ f
\]

The expressions are mathematically equivalent, assuming compatible function types and the usual pure-function semantics.

For Gardel, I would begin with a standard composition function or operator only if the language already has a suitable way to represent and pass functions.

I would **not** introduce special syntax merely because other functional languages have it. The library should establish the concept first; syntax can be considered later if experience demonstrates a real usability benefit.

---

# 7. `SET[A]` and `MAP[K, V]`: important, but not the first step

Sets and maps are useful standard-library abstractions, but they introduce additional semantic requirements.

A set needs a definition of element equality. A map needs key equality and a rule for handling duplicate keys.

Depending on the implementation, they may also require ordering or hashing.

Before adding these modules, establish what Gardel supports for:
- equality on arbitrary generic types;
- ordering and comparison;
- generic constraints or interfaces;
- immutable updates;
- representation independence.

The supplied specification does not establish all these capabilities, so I would not assume a particular implementation strategy.

### My recommendation

Add `SET` and `MAP` after `OPTION`, `RESULT`, and `SEQ` have stable semantics and laws.

They are important for real programs, but a correct `map` function over sequences is not the same thing as a `MAP[K, V]` associative collection. The two concepts should have distinct names and contracts.

---

# 8. The real differentiator: a library of algebraic laws

This is the part I would prioritize most strongly for Gardel.

Most standard libraries document the behavior of their functions. A library designed for verification should go further: it should make the relationships between operations explicit.

For example, a specification for sequence concatenation should establish the abstract relationship:

\[
\operatorname{model}(a \mathbin{+\!\!+} b)
=
\operatorname{model}(a)
\mathbin{+\!\!+}
\operatorname{model}(b)
\]

The notation \(+\!\!+\) here means sequence concatenation, not numeric addition.

Similarly, `map` should preserve the sequence structure at the abstract level:

\[
\operatorname{model}(\operatorname{map}(f,s))
=
\operatorname{map}(f,\operatorname{model}(s))
\]

These equations connect the concrete API to its mathematical model.

They can support three activities:

1. **Verification:** prove that implementations satisfy their contracts.
2. **Program derivation:** construct an implementation from a desired mathematical behavior.
3. **Optimization:** replace an expression with an equivalent one while preserving its specified behavior.

For example, the `map` composition law gives a mathematical basis for replacing two successive transformations with one composed transformation.

Whether a particular rewrite is beneficial still depends on the implementation and cost model. Equivalence alone does not guarantee better performance.

## A proposed law catalogue

| Module | Laws worth specifying |
|---|---|
| `OPTION` | Identity and composition of `map`; preservation of absence |
| `RESULT` | Identity and composition of `map`; propagation of failure through `and_then` |
| `SEQ` | Concatenation identity and associativity; map identity and composition; length of concatenation |
| `STRING` | Concatenation identity and associativity; length semantics; conversion round trips where defined |
| `BIGNAT` | Arithmetic identities; commutativity and associativity where applicable; correspondence between operations and the mathematical model |
| `SET` | Membership, insertion, and union laws |
| `MAP` | Lookup and update laws; key replacement semantics |

The laws should be specified in terms of abstract behavior wherever possible, not implementation details.

That is particularly important for `SEQ`, where two different tree representations may denote the same sequence.

---

# 9. A practical implementation roadmap

I would deliver the library incrementally, with each phase producing a usable and verifiable result.

### Phase 1 — Correct the foundations

**Goal:** Ensure that the existing abstractions have sound mathematical meanings.

- Correct the `BIGNAT.model` definition.
- Clarify the byte/character semantics of `STRING`.
- Define the numeric overflow semantics relevant to `SEQ.length`.
- Establish the relationship between `SEQ` and `LIST`.
- Add or strengthen model-based postconditions and representation invariants.

This phase should come before API expansion. Otherwise, new functions may inherit incorrect assumptions from the existing models.

### Phase 2 — Complete the functional core

**Goal:** Make the existing abstractions useful for ordinary functional programming.

Add:
- `OPTION.map`, `OPTION.and_then`, and `OPTION.unwrap_or`.
- `RESULT` with success and failure propagation.
- `SEQ.is_empty`, `SEQ.map`, and `SEQ.filter`.
- `SEQ.fold_left` and `SEQ.fold_right`.
- The corresponding algebraic laws.

At the end of this phase, a programmer should be able to write useful functional transformations without introducing new data structures for every task.

### Phase 3 — Add reusable algorithms

**Goal:** Reduce duplication and support more complex transformations.

Add:
- sequence reversal, prefix and suffix operations, and common queries;
- function composition;
- more complete string operations;
- `BIGNAT` arithmetic, specified against the corrected mathematical model.

For generic collection operations, first establish the equality and comparison capabilities required by their contracts.

### Phase 4 — Expand the data abstractions

**Goal:** Support a broader range of applications.

Add `SET` and `MAP`, followed by more advanced collection algorithms as needed.

Each new abstraction should come with:
- an abstract model;
- a representation invariant;
- contracts for public operations;
- algebraic laws;
- tests and, where supported, machine-checked proofs.

---

# 10. What I would make the official Gardel Core Library

If the goal is a small, focused initial release, I would define the essential scope as follows.

| Component | Minimum deliverable |
|---|---|
| `OPTION[A]` | Existing constructors and accessors, plus `map`, `and_then`, and a default operation |
| `RESULT[E, A]` | Explicit success/failure representation and composition |
| `SEQ[A]` | Existing constructors and concatenation, plus `map`, `filter`, folds, and queries |
| `STRING` | Clearly defined text representation, concatenation, equality, and validated UTF-8 operations |
| `BIGNAT` | Correct mathematical model, canonical representation rules, and basic arithmetic |
| Functional laws | A catalogue of laws connecting public operations to their abstract semantics |
| Verification examples | Small, representative proofs demonstrating how to use the contracts and laws |

I would leave `SET`, `MAP`, advanced collection operations, and new syntactic constructs for later releases.

## The key design decision

Gardel should not compete with other languages by having the largest standard library. It should aim to provide a **small library whose abstractions, contracts, and algebraic laws work together**.

That has a direct benefit for AI-assisted programming. An agent can select a library operation, use its contract to understand its behavior, compose it with other operations, and rely on established laws to justify transformations.

The resulting workflow is:

**Specify behavior → compose existing operations → implement or derive the missing operation → verify its contract → optimize using proven laws.**

My first milestone would therefore be **a verified functional core consisting of `OPTION`, `RESULT`, and `SEQ`, backed by an explicit catalogue of algebraic laws**. In parallel, I would correct the existing `BIGNAT` and `STRING` models, because a larger library is only as reliable as the semantics of its foundations.