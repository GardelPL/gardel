# The Gardel Programming Guide

Welcome to the definitive 
   - [A Package for Terminal Interfaces: `gtui`](#a-package-for-terminal-interfaces-gtui)guide to **Gardel**, a pure, total, statically verified programming language built on **Tony Hoare's axiomatic semantics**. Every routine is a triple $\{P\}\;C\;\{Q\}$, proved at compile time by Boogie and Z3 and then erased. Gardel's contract syntax comes from Eiffel, its style of automated proof from Dafny, and its model of parallelism from Bend.

This guide is written for two audiences:
1. **Software engineers** seeking mathematical certainty, zero-cost static contracts, and automatic lock-free parallelism.
2. **AI coding agents (LLMs)** generating, evaluating, and repairing high-integrity code via structured SMT diagnostics.

---

## Table of Contents

- [1. Philosophy and Mental Model](#1-philosophy-and-mental-model)
  - [The Foundation: Axiomatic Semantics](#the-foundation-axiomatic-semantics)
  - [What Gardel Borrows](#what-gardel-borrows)
  - [The Core Invariant: Total Purity and Determinism](#the-core-invariant-total-purity-and-determinism)
  - [Contracts as Static Proofs, Not Runtime Assertions](#contracts-as-static-proofs-not-runtime-assertions)
- [2. Getting Started: Hello World](#2-getting-started-hello-world)
  - [Building from Source](#building-from-source)
  - [Your First Program](#your-first-program)
  - [Running and Checking Code](#running-and-checking-code)
- [3. Primitives and 64-Bit Numerics](#3-primitives-and-64-bit-numerics)
  - [Machine Types vs Mathematical Integers](#machine-types-vs-mathematical-integers)
  - [Checked Operators & Overflow Proofs](#checked-operators--overflow-proofs)
  - [Wrapping and Bitwise Operators](#wrapping-and-bitwise-operators)
  - [Deterministic Floating Point (`F64`)](#deterministic-floating-point-f64)
  - [Text: `CHAR` and `STRING`](#text-char-and-string)
- [4. Statements, Blocks, and Local Proof State](#4-statements-blocks-and-local-proof-state)
  - [Immutable `let` Bindings](#immutable-let-bindings)
  - [Intermediate Verifier Directives: `check`, `assume`, and `reveal`](#intermediate-verifier-directives-check-assume-and-reveal)
- [5. Representations and Pattern Matching](#5-representations-and-pattern-matching)
  - [Product Types (Records) and Functional Updates](#product-types-records-and-functional-updates)
  - [Sum Types (Algebraic Variants)](#sum-types-algebraic-variants)
  - [Refinement-Aware Pattern Matching](#refinement-aware-pattern-matching)
- [6. Contracts: Hoare Triples in Practice](#6-contracts-hoare-triples-in-practice)
  - [Preconditions: `require`](#preconditions-require)
  - [Postconditions: `ensure`](#postconditions-ensure)
  - [The Roles of `Current` and `Result`](#the-roles-of-current-and-result)
  - [Tagged Predicates and AI Traceability](#tagged-predicates-and-ai-traceability)
- [7. Modules, Visibility, and Verification Firewalls](#7-modules-visibility-and-verification-firewalls)
  - [Module Structure (`representation`, `create`, `feature`, `specify`)](#module-structure-representation-create-feature-specify)
  - [The 5 Visibility Rules](#the-5-visibility-rules)
  - [Uniform Access Principle](#uniform-access-principle)
- [8. Invariants as Refinement Types](#8-invariants-as-refinement-types)
  - [Class Invariants: Invariants Are Types](#class-invariants-invariants-are-types)
  - [Construction-Site Verification](#construction-site-verification)
  - [Hereditary Invariants and Destructuring](#hereditary-invariants-and-destructuring)
- [9. Total Recursion and Termination Metrics](#9-total-recursion-and-termination-metrics)
  - [The Totality Requirement (No Infinite Loops)](#the-totality-requirement-no-infinite-loops)
  - [Structural Induction with `variant`](#structural-induction-with-variant)
  - [Well-Founded Arithmetic Metrics](#well-founded-arithmetic-metrics)
  - [Lexicographic Variants](#lexicographic-variants)
  - [Mutual Recursion](#mutual-recursion)
  - [No Recursion Through Function Values](#no-recursion-through-function-values)
  - [Loops That Run Fast in C](#loops-that-run-fast-in-c)
- [10. Automatic Deterministic Parallelism](#10-automatic-deterministic-parallelism)
  - [Divide-and-Conquer Parallel Fork-Join](#divide-and-conquer-parallel-fork-join)
  - [Church-Rosser Confluence](#church-rosser-confluence)
  - [The `SEQ[A]` Balanced Tree Collection](#the-seqa-balanced-tree-collection)
  - [Backends Today](#backends-today)
- [11. Higher-Order Functions and Closures](#11-higher-order-functions-and-closures)
  - [Function Signatures and Refined Types](#function-signatures-and-refined-types)
  - [Lambda Syntax (`fn`)](#lambda-syntax-fn)
  - [Compile-Time Closure Conversion (Lambda Lifting)](#compile-time-closure-conversion-lambda-lifting)
- [12. Concepts, Laws, and Mathematical Conformance](#12-concepts-laws-and-mathematical-conformance)
  - [Declaring a `concept` and Laws](#declaring-a-concept-and-laws)
  - [Proving Laws in `specify`](#proving-laws-in-specify)
  - [Inductive Law Proofs](#inductive-law-proofs)
  - [The Equality Hierarchy: `=`, `!=`, `equals`, and Models](#the-equality-hierarchy---equals-and-models)
  - [Total Orders (`ORD`)](#total-orders-ord)
  - [Conditional Conformance (`where`)](#conditional-conformance-where)
- [13. The Linear IO Firewall](#13-the-linear-io-firewall)
  - [Why Pure Modules Forbid Effects](#why-pure-modules-forbid-effects)
  - [Linear `IO[A]` Actions and `with IO`](#linear-ioa-actions-and-with-io)
  - [Reading Input: `IO.read_line`](#reading-input-ioread_line)
  - [The Time and Random Bytes: `IO.now`, `IO.random`](#the-time-and-random-bytes-ionow-iorandom)
  - [Logging and the Environment: `IO.log`, `IO.env`](#logging-and-the-environment-iolog-ioenv)
  - [Boundary Ingestion: Untrusted Data to Verified ADTs](#boundary-ingestion-untrusted-data-to-verified-adts)
- [14. Reactive Programs](#14-reactive-programs)
  - [Two Total Routines](#two-total-routines)
  - [The State Proves Itself](#the-state-proves-itself)
  - [What Is and Is Not Guaranteed](#what-is-and-is-not-guaranteed)
  - [Scripted Runs, Record and Replay](#scripted-runs-record-and-replay)
- [15. SMT Proofs & AI Agent Verification Workflow](#15-smt-proofs--ai-agent-verification-workflow)
  - [The Boogie / Z3 Verification Engine](#the-boogie--z3-verification-engine)
  - [The AI Autonomous Repair Loop (`--json-diagnostics`)](#the-ai-autonomous-repair-loop---json-diagnostics)
  - [Reading Counterexample Models](#reading-counterexample-models)
  - [Repair Patterns for LLMs](#repair-patterns-for-llms)
- [16. Complete Real-World Examples](#16-complete-real-world-examples)
  - [Example A: `BOUNDED_STACK[A]`](#example-a-bounded_stacka)
  - [Example B: Verified Parallel Tree Construction & Maximum](#example-b-verified-parallel-tree-construction--maximum)
  - [Example C: Peano Numbers with Formally Verified Total Ordering](#example-c-peano-numbers-with-formally-verified-total-ordering)
- [17. Projects and the Standard Prelude](#17-projects-and-the-standard-prelude)
  - [The Prelude Is Always There](#the-prelude-is-always-there)
  - [Multi-File Projects](#multi-file-projects)
  - [Project Manifests (`Gardel.toml`)](#project-manifests-gardeltoml)
  - [Dependencies](#dependencies)
  - [A Package for JSON: `gjson`](#a-package-for-json-gjson)
  - [Testing a Package: `gardelc test` and `gtest`](#testing-a-package-gardelc-test-and-gtest)
  - [Mutating a Package: `gardelc mutate`](#mutating-a-package-gardelc-mutate)
  - [Checking a New Version](#checking-a-new-version)
  - [Name Resolution](#name-resolution)
  - [The Standard Library Sources (`stdlib/`)](#the-standard-library-sources-stdlib)
- [18. Compiler CLI Cheatsheet & Grammar Reference](#18-compiler-cli-cheatsheet--grammar-reference)
  - [CLI Commands](#cli-commands)
  - [Formal Grammar (EBNF Summary)](#formal-grammar-ebnf-summary)
  - [Comparison Matrix: Gardel vs Bend vs Eiffel vs Rust vs Dafny](#comparison-matrix-gardel-vs-bend-vs-eiffel-vs-rust-vs-dafny)

---

## 1. Philosophy and Mental Model

### The Foundation: Axiomatic Semantics

Gardel's core is the axiomatic semantics Tony Hoare published in 1969. The meaning of a program is what can be proved about it, stated as a triple:

$$\{P\}\;C\;\{Q\}$$

If $P$ holds before $C$ runs, then $Q$ holds after it. Each part of a routine is a part of that triple:

| Axiomatic semantics | In Gardel | Proved by |
| :--- | :--- | :--- |
| Precondition $\{P\}$ | `require` | Every caller, at the call site |
| Command $C$ | `do` | Not applicable: this is the code being verified |
| Postcondition $\{Q\}$ | `ensure` | The routine, assuming $P$ |
| Data invariant | `invariant` | Every routine that builds a value of the type |
| Termination, for total correctness | `variant` | Every recursive routine |

A call follows Hoare's rule for procedures: prove the callee's precondition, then use its postcondition. Boogie turns each triple into verification conditions and Z3 discharges them. A triple that cannot be proved stops the build.

### What Gardel Borrows

- **Eiffel's Design by Contract** is one implementation of these ideas in a programming language. Gardel borrows its readable vocabulary: `require`, `ensure`, `invariant`, `Current`, `Result`, and selective export. Eiffel usually monitors assertions at run time, while Gardel proves them.
- **Dafny** showed that SMT-backed proofs, ghost specifications and `variant`-style termination measures work in everyday code.
- **Bend** is the model for parallelism without locks, and an intended backend. See [Backends Today](#backends-today) for what runs now.

### The Core Invariant: Total Purity and Determinism

In Gardel:
1. **Values are immutable.** There are no variables that change over time, no pointers, and no in-place mutation.
2. **All routines are total functions.** A routine cannot loop forever, crash with an unhandled exception, or return undefined. Given valid inputs meeting its `require` clause, it is mathematically proven to terminate and produce a result satisfying its `ensure` clause.
3. **Strict Church-Rosser confluence.** Because every expression is purely functional and total, evaluating independent expressions sequentially, interleaved, or in parallel produces the **exact same bit-for-bit result** for discrete types.

### Contracts as Static Proofs, Not Runtime Assertions

In languages like Python, C++, or standard Eiffel, contracts are often run as debug assertions (`assert x > 0`) which slow down execution and get turned off in production.

In Gardel, **contracts are never executed at runtime**:
- Contracts are translated into mathematical first-order logic formulas discharged by the **Boogie** and **Z3** automated theorem provers during compilation. Boogie is Gardel's only verifier: there is no second, unchecked backend whose verdict could disagree.
- Once verified, contracts are **completely erased**. The resulting C11 / Bend code has zero runtime assertion checks, zero overhead, and maximum speed.

---

## 2. Getting Started: Hello World

### Building from Source

Before compiling and verifying Gardel programs, you need to build the `gardelc` compiler binary from source. The compiler is written in Rust and has zero external crate dependencies.

#### 1. Prerequisites

- **Rust & Cargo:** Rust toolchain (Rust 2024 edition / 1.85+ recommended). Install via [rustup.rs](https://rustup.rs/):
  ```bash
  # Linux / macOS:
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

  # Windows:
  # Download and run rustup-init.exe from https://rustup.rs/
  ```
- **C11 Compiler (Optional, for native CPU binaries):** GCC or Clang with OpenMP support (e.g., `gcc -fopenmp`).
- **Bend (not needed):** `gardelc` writes a `.bend` file on each run, but no emitted file has been executed yet. See [Backends Today](#backends-today).

#### 2. Compiling the Compiler

Clone the Gardel repository and build the release binary:

```bash
# Clone the repository
git clone https://github.com/gardel-lang/gardel.git
cd gardel

# Build the release binary with Cargo
cargo build --release --manifest-path gardelc/Cargo.toml

# Or navigate directly into the compiler crate directory:
cd gardelc
cargo build --release
```

#### 3. Running the Test Suite

Confirm that all compiler components (parser, visibility rules, concept laws, Boogie verification conditions, and C11/Bend emitters) pass:

```bash
cargo test
# Output: test result: ok. 164 passed; 0 failed
```

#### 4. Installing `gardelc` to Your PATH

Install the binary directly into your Cargo binary directory (which is usually in your system `PATH`):

```bash
# From within the gardelc directory:
cargo install --path .

# Or from the repository root:
cargo install --path gardelc
```

Alternatively, copy the compiled binary to a folder in your `PATH`:

```bash
# Linux / macOS:
sudo cp target/release/gardelc /usr/local/bin/

# Windows (PowerShell):
Copy-Item target\release\gardelc.exe "$HOME\.cargo\bin\"
```

#### 5. Verifying the Installation

Test that `gardelc` is installed and verify the entire built-in Standard Prelude under strict mode:

```bash
gardelc --stdlib --strict
```

You should see (run from the repository root; from `gardelc/` the tree is shown as `../stdlib`):
```text
[stdlib] Verifying the source tree `stdlib` (12 files).
[0/5] Bootstrap mode (--no-prelude): Standard Prelude not injected
[1/5] Linked project of 12 files into one flat namespace (§4.2): 9 modules [...], 3 concepts ["EQ", "CANONICAL_EQ", "ORD"], fuel: 2
[2/5] Checked §4 Visibility, §9 Concept Laws, §11 IO Boundary & §12 Direct Verification:
      0 errors, 0 warnings — all visibility, concept laws, IO linearity, overflow, and contract obligations satisfied.
[3/5] Emitted direct Boogie VCs & fuel axioms -> build/....bpl (one per module; ...)
[4/5] Emitted monomorphized Bend v2 target    -> build/....bend
[5/5] Emitted C11 runtime target              -> build/....c
```

---

### Your First Program

Programs that interact with the outside world (printing to standard output, reading files) are defined inside a `program` module. The entry point is `main`, which returns an `IO` action:

```eiffel
program HELLO

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line("Hello, world! Welcome to Gardel.")
        IO.exit(0)
      end
    end

end
```

### Running and Checking Code

The Gardel compiler is `gardelc`:

No imports are needed: the Standard Prelude (`STRING`, `SEQ`, `OPTION`, `RESULT`, `IO_ERROR`, ...) is available in every program (§16).

```bash
# Verify all contracts, concept laws, and proofs statically (emits Boogie/Bend/C11 to build/):
gardelc hello.gdl

# Strict verification (rejects any unproven 'assume' blocks):
gardelc hello.gdl --strict

# Compile, verify, and run immediately:
gardelc hello.gdl --run

# Emit structured JSON diagnostics for AI repair loops:
gardelc hello.gdl --json-diagnostics

# Inspect the public Eiffel-style contract interface of a module (like Eiffel 'short'):
gardelc --inspect hello.gdl

# Run the emitted C11 binary (in build/):
gcc -O3 -fopenmp build/hello.c -o hello    # a reactive program on Windows also needs -lws2_32
./hello
```

---

## 3. Primitives and 64-Bit Numerics

### Machine Types vs Mathematical Integers

Gardel bridges the gap between hardware efficiency and mathematical logic by separating **runtime machine types** from **ghost specification types**:

| Type | Kind | Range | Purpose |
| :--- | :--- | :--- | :--- |
| `U64` (alias `NAT`) | Runtime | $0 \dots 2^{64}-1$ | Unsigned 64-bit machine integer |
| `I64` (alias `INT`) | Runtime | $-2^{63} \dots 2^{63}-1$ | Signed 64-bit two's complement integer |
| `F64` | Runtime | IEEE-754 64-bit float | High-precision floating point |
| `BOOL` | Runtime | `True`, `False` | Boolean values |
| `CHAR` | Runtime | Unicode scalar value | UTF-32 scalar character |
| `UNIT` (alias `()`) | Runtime | `()` | Unit singleton (erased to 0 bytes) |
| `MATH_INT` | Ghost | $-\infty \dots +\infty$ | Unbounded mathematical integer (contracts only) |

Inside contracts (`require`, `ensure`, `invariant`), runtime numbers **automatically lift to `MATH_INT`**. For example, in an `ensure` clause, `Result = a + b` means true unbounded mathematical addition.

### Checked Operators & Overflow Proofs

Arithmetic operators generate static proof obligations, each checked against the range of its operands' type, on the path where the operation runs:

| Operation | What you must prove |
| :--- | :--- |
| `a + b`, `a - b`, `a * b` | The mathematical result fits the type: $[0, 2^{64}-1]$ for `U64`, $[-2^{63}, 2^{63}-1]$ for `I64` |
| `a / b`, `a % b` | $b \ne 0$, and for `I64`, not `I64.min / -1` or `I64.min % -1`, which trap on real hardware |
| `-a` | The result fits: `a` is not `I64.min` for `I64`, and `a = 0` for `U64` |

`/` truncates toward zero and `%` takes the sign of the dividend, as in C, so `(-7) / 2 = -3` and `(-7) % 2 = -1`. This holds in running code and in contracts.

A guard protects what it guards. `x != 0 and 10 / x > 1` verifies, because `and`, `or` and `implies` evaluate their right side only when the left allows it. So does `id(if x > 0 then 10 / x else 0 end)`.

If the compiler cannot prove an operation will never overflow or divide by zero, compilation fails!

```eiffel
-- WILL NOT COMPILE: lo + hi can overflow U64 when hi is large!
bad_midpoint(lo: NAT, hi: NAT): NAT
  require
    ordered: lo <= hi
  do
    (lo + hi) / 2
  end

-- VERIFIES CLEANLY: hi - lo cannot underflow because lo <= hi;
-- diff / 2 <= hi - lo; lo + diff / 2 <= hi <= NAT.max.
midpoint(lo: NAT, hi: NAT): NAT
  require
    ordered: lo <= hi
  do
    let diff = hi - lo
    lo + diff / 2
  ensure
    in_range: lo <= Result and Result <= hi
  end
```

### Wrapping and Bitwise Operators

When implementing cryptographic algorithms, hashing, or low-level encodings where overflow is intentional:
- Use wrapping operators: `+%`, `-%`, `*%`. These are total and defined modulo $2^{64}$, and on `I64` they are two's complement.
- Use bitwise operators: `&`, `|`, `^`, `<<`, `>>`. They are defined on `U64` only, and rejected on `I64` (`bitwise_on_signed`). Shifts require proving `shift_amount < 64`.

### Deterministic Floating Point (`F64`)

Floating-point math in Gardel follows strict syntactic expression-tree evaluation order:
- Compiler re-association (such as `-ffast-math`) and uncoordinated fused multiply-add (`fma`) synthesis are prohibited.
- For discrete types (`NAT`, `INT`, `BOOL`, ADTs), Church-Rosser confluence guarantees exact bit-for-bit equivalence regardless of parallel thread scheduling.

### Text: `CHAR` and `STRING`

`CHAR` is a Unicode scalar value. `STRING` is a prelude module (`stdlib/text/string.gdl`) whose ghost model is `chars: LIST[CHAR]`.

A `STRING` is well-formed UTF-8, and every index counts scalars, not bytes: `"café".code_point_count = 4` while its `byte_length` is 5. Raw input arrives as `BYTES`, and becomes text only through `STRING.from_utf8`, which returns an error for malformed input.

String literals are ordinary verified values. `"Hi"` is defined as a chain of verified prelude calls (`STRING.make_empty(2).push_ascii('i').push_ascii('H')`), so the verifier knows `"Hi".chars` and `"Hi".byte_length = 2` with no extra trust. The compiler enforces these rules:

| Code | Rejected | Use instead |
| :--- | :--- | :--- |
| `E0302` / `E0304` | Malformed escapes / surrogate code points | Valid escapes |
| `E0311` | `=` with a `STRING` operand (`STRING` is not `CANONICAL_EQ`: two strings with different capacities are equal text) | `a.equals(b)` or `a.chars = b.chars` |
| `E0312` | `+` on `STRING` | Print the pieces with separate `IO.print` calls |
| `E0313` | String literals as `match` patterns | `if s.equals("...") then ... else ... end` |

#### Working with text

Every operation states its result on `chars` (or, for `BYTES`, on `model`), so a client can prove what it gets:

| On `STRING` | Gives |
| :--- | :--- |
| `s.slice(lo, hi)` | Scalars `lo` up to, not including, `hi`. Needs `lo <= hi <= s.code_point_count`. |
| `s.find(t, from)` | `Some(i)`, the first index at or after `from` where `t` occurs, or `None` when there is none. |
| `s.find_char(c, from)` | The same for one `CHAR`. |
| `s.starts_with(p)`, `s.ends_with(p)` | Whether `p` is at the start or the end. |
| `s.trim` | `s` without ASCII white space at either end. |
| `s.split(c)` | The parts between each `c`: `count_char(c, 0) + 1` of them. |
| `s.to_chars` | The scalars as a `LIST[CHAR]`, to walk. C decodes the bytes once. |
| `STRING.from_char(c)` | The text of one scalar. |
| `a.less_than(b)` | Byte order of the UTF-8 form, so `STRING` is a key for a generic `[K: ORD]`. It is not a collation: `"Z"` comes before `"a"`. |

The contract of `split` gives the number of parts and one step of the scan at a time, which is enough for laws such as `TEXT_FORMAT.join_split` below, but not the text of a given part. When a proof depends on one piece, cut the text with `find_char` or `find` and `slice`, whose contracts are exact:

```eiffel
-- The text before the first `:`, or the whole line when there is none.
header_name(line: STRING): STRING
  do
    match line.find_char(':', 0)
      case Some(i) then line.slice(0, i)
      case None then line
    end
  ensure
    shorter: Result.code_point_count <= line.code_point_count
  end
```

`BYTES` has `concat`, `slice`, `starts_with`, `index_of_byte` and `index_of`, with the same contracts. `slice` shares the buffer, so it costs no copy. `concat` copies both sides, so to build a buffer piece by piece use `BYTES_BUILDER`:

```eiffel
let out = BYTES_BUILDER.empty.add_text("HTTP/1.1 ").add_text("200 OK").add_byte(13).add_byte(10)
out.to_bytes
```

Each `add` requires the total length to fit a `U64` (`fits`), so text of unknown length needs a bound first, such as `if s.byte_length <= 1000000 then ...`.

`NUMBER_TEXT` turns numbers into decimal text and back. Parsing is total, and a value too large for its type is `Error(OutOfRange)`, never a wrapped number:

```eiffel
NUMBER_TEXT.nat_to_text(42)            -- "42"
NUMBER_TEXT.parse_nat("18446744073709551616")  -- Error(OutOfRange)
NUMBER_TEXT.parse_i64("-7")            -- Ok(-7)
NUMBER_TEXT.parse_nat("4x")            -- Error(NotADigit(1))
```

The round trips are proven laws, so this verifies with no extra work:

```eiffel
read_back(n: NAT): RESULT[NUMBER_ERROR, NAT]
  do
    NUMBER_TEXT.parse_nat(NUMBER_TEXT.nat_to_text(n))
  ensure
    same: Result = Ok(n)
  end
```

`NUMBER_TEXT.digits(n, rest)` puts the decimal digits of `n` in front of the byte list `rest`, as ASCII codes. A client cannot see its body, so the law `digits_step` gives one step of it: `digits(n, rest) = rest.prepend(48 + n)` when `n < 10`, and `digits(n / 10, rest.prepend(48 + n % 10))` otherwise. It has no trigger, so a proof calls it where it needs it, as in `check s: NUMBER_TEXT.digits_step(n, rest) end`.

#### Building, formatting and encoding text

`STRING_BUILDER` builds text by appending, as `BYTES_BUILDER` builds bytes. Each `add` keeps the part, and `to_string` copies every part once, so building is linear where a chain of `concat` would copy the text again at each step:

```eiffel
greeting(name: STRING, visits: NAT): STRING
  require
    short: name.byte_length <= 1000
  do
    STRING_BUILDER.empty.add("Hello, ").add(name).add_char('!')
      .add(" Visit #").add_nat(visits).to_string
  end
```

Its model is the scalars added so far: `b.add(s).model = b.model.append(s.chars)`, and `to_string.chars` is the model. Each `add` requires the text to fit a `U64` (`fits`), as `BYTES_BUILDER`'s does. `add_nat` and `add_i64` add at most 20 bytes, and `add_char(c)` adds `STRING.char_width(c)`, one to four.

`TEXT_FORMAT` has the usual formatting, each stated on `chars`:

| Routine | Gives |
| :--- | :--- |
| `pad_left(s, width, c)`, `pad_right(s, width, c)` | `s` with copies of `c` before or after it, up to `width` scalars. A longer `s` is kept as it is. |
| `repeat(s, n)` | `n` copies of `s`. Needs `n * s.byte_length` to fit. |
| `join(parts, sep)` | The parts with `sep` between each two. Needs `joined_bytes(parts, sep)` to fit. |
| `to_lower(s)`, `to_upper(s)` | ASCII `A` to `Z` as `a` to `z`, or back. Every other scalar is kept, so `é` stays `é`, and the byte length is the same. |

`split`'s contract now states each step, and the law `join_split` states that joining the parts again gives the text back, for any text and separator:

```eiffel
rejoin(s: STRING, c: CHAR): STRING
  do
    TEXT_FORMAT.join(s.split(c), STRING.from_char(c))
  ensure
    same: Result.equals(s)
  end
```

The law `join_split_fits` gives the call its `fits`. For parts of unknown length and another separator, test it at run time with `join_fits`, whose contract is that `fits`: `if TEXT_FORMAT.join_fits(parts, ", ") then TEXT_FORMAT.join(parts, ", ") else ... end`.

Three codecs turn bytes into text and back. Encoding is total and decoding returns a `RESULT[CODEC_ERROR, _]`, whose errors are `BadLength`, `BadSymbol(at)` with the byte offset, and `NotText`:

| Module | Encodes | Decodes |
| :--- | :--- | :--- |
| `HEX` | `encode(b)`: two lowercase digits per byte | `decode(s)`: either case |
| `BASE64` | `encode(b)`: RFC 4648's standard alphabet, padded. `encode_with(b, url, pad)` picks the URL-safe alphabet and whether to pad. | `decode(s)`, `decode_with(s, url, pad)`. A symbol outside the alphabet, padding out of place, and unused bits that are not zero are refused, so a text decodes in at most one way. |
| `PERCENT` | `encode(s)`: RFC 3986's unreserved bytes as they are, every other byte as `%` and two uppercase digits | `decode(s)`: `NotText` if the bytes are not UTF-8. `+` is not a space here. |

Each round trip is a proven law: `HEX.round_trip`, `BASE64.round_trip` (for both alphabets, padded or not) and `PERCENT.round_trip`. A client gets it with no extra work:

```eiffel
back(b: BYTES): BOOL
  require
    short: b.length <= 1000000
  do
    True
  ensure
    same: BASE64.decode(BASE64.encode(b)).is_ok and BASE64.decode(BASE64.encode(b)).get.model = b.model
  end
```

The decoded value is the same bytes or text, so the law states that its `model` (or `equals`) is the same. `=` on a `BYTES` or a `STRING` compares the representation, which a client does not see.

`NUMBER_TEXT.nat_to_hex(n)` writes `n` in lowercase hex, and `parse_hex(s)` reads hex of any length and either case, with `OutOfRange` above `U64`. The law `hex_round_trip` states that one reads back the other.

`MAP[K, V]` maps keys to values. `K` must be ordered (`ORD`): `STRING` and every number type are. Its representation is hidden, so what a client can prove comes from its laws, and each law reaches a client by itself:

```eiffel
facts(m: MAP[NAT, NAT], x: NAT): BOOL
  do
    True
  ensure
    found: m.put(1, x).get(1) = Some(x)
    kept: m.put(1, x).get(2) = m.get(2)
    gone: m.put(1, x).remove(1).get(1) = None
  end
```

`keys` come out in ascending order and `fold(init, fn(acc: B, k: K, v: V): B do ... end)` visits them in that order. Each operation walks the map, so it takes time linear in its size. With `STRING` values, compare what `get` gives with `.equals`, as `=` on `OPTION[STRING]` is `E0311`.

Other laws count the keys:
- `m.put(k, v).size` is at most `m.size + 1`, and is `m.size + 1` when `not m.has(k)`. `m.remove(k).size` is at most `m.size`, and is `m.size` when `not m.has(k)`.
- When `m.has(k)`, `m.put(k, v).size = m.size` and `m.remove(k).size + 1 = m.size`. These two need the premise `agrees` (see `binary_search` below), which a `NAT`, `I64` or `U64` client states in one `check`, and which does not prove for `STRING`.

`fold_put_last` says that a new key above every key is the last a fold meets: `m.put(k, v).fold(init, f) = f(m.fold(init, f), k, v)` when `not m.has(k)` and `m.size = 0 or LIST_OPS.at(m.keys, m.size - 1) < k`. `keys_put_last` says that `k` is then the last key, so a chain of `put`s in order can be folded step by step.

`m.filter(fn(k: K, v: V): BOOL do ... end)` keeps the entries the function holds for, and `m.map(fn(k: K, v: V): W do ... end)` gives a map with the same keys and the function's values:

```eiffel
large(m: MAP[NAT, NAT], k: NAT): BOOL
  require
    present: m.has(k)
    small: m.get(k).get <= 10
  do
    check
      agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)
    end
    True
  ensure
    gone: m.filter(fn(i: NAT, v: NAT): BOOL do v > 10 end).get(k) = None
  end
```

- `get_filter_kept`, `get_filter_holds` and `get_filter_absent` need no `agrees`, so they hold for `STRING` keys too. The first two need a function that cannot tell equal keys apart (`respects`), as one that looks at the value alone. `get_filter_dropped`, as above, needs `agrees`.
- `size_filter_parts`: the sizes of two filters add up to `m.size` when one function holds where the other does not.
- `get_map`: `m.map(f).get(k)` is `Some(f(k, v))` when `m.get(k) = Some(v)`, and `None` when `m` has no `k`. It needs `respects`.
- `keys_present`: the map has each of its `keys`. `values_at`: the value of the key at place `i` of `keys` is at place `i` of `values`. It needs `agrees`.

To change between `NAT` and `I64`, use `NAT.from_i64(i)`, which needs `i >= 0`, and `I64.from_nat(n)`, which needs `n <= 9223372036854775807`. Both are proof obligations (`in_range`), not run-time checks.

---

## 4. Statements, Blocks, and Local Proof State

### Immutable `let` Bindings

Function bodies and branch targets in Gardel are `Block` structures consisting of zero or more statements followed by a trailing expression:

```eiffel
calculate_tax(income: NAT, deduction: NAT): NAT
  require
    valid: deduction <= income
  do
    let taxable = income - deduction
    let rate = 20
    (taxable * rate) / 100
  ensure
    reasonable: Result <= income
  end
```

`let` bindings are immutable. When you write `let x = e`, the verifier immediately adds the equality `x = e` to the active proof context for all subsequent statements in the block.

### Intermediate Verifier Directives: `check`, `assume`, and `reveal`

When SMT solvers need guidance on complex mathematical steps:

#### 1. `check Tag: Predicate end`
Directs the solver to prove a local lemma before continuing. If proved, it enters the proof context:
```eiffel
let doubled = x + x
check even_parity: doubled % 2 = 0 end
doubled
```

#### 2. `assume Tag: Predicate end`
Introduces an assertion into the solver's context without proof.
> [!CAUTION]
> `assume` bypasses static safety. `gardelc --audit` flags every `assume` in your codebase, and `gardelc --strict` rejects code containing any `assume`.

#### 3. `reveal query_name`
By default, recursive functions are treated as opaque black boxes (unfolded only to fuel depth 2) to keep SMT solving instantaneous. `reveal f` tells the solver to unfold function `f` completely within the current block.

---

## 5. Representations and Pattern Matching

### Product Types (Records) and Functional Updates

A product type defines a record with named fields:

```eiffel
module POINT

representation {ANY}
  x: I64
  y: I64

create {ANY}
  origin: POINT
    do
      { x = 0, y = 0 }
    end

feature {ANY}
  translate_x(dx: I64): POINT
    do
      { Current with x = Current.x + dx }
    end

end
```

- **Record literal**: `{ x = 10, y = 20 }`
- **Functional update**: `{ Current with x = new_x }` creates a new record copying unchanged fields.

### Sum Types (Algebraic Variants)

Sum types (tagged unions / algebraic variants) are declared using the pipe `|` syntax:

```eiffel
module OPTION[A]

representation {ANY}
  | None
  | Some(value: A)

feature {ANY}
  is_some: BOOL
    do
      match Current
        case None    then False
        case Some(_) then True
      end
    end
end
```

A field of a number type keeps the range of its type. In `| Box(n: NAT, i: I64)`, the `n` that `case Box(n, i)` binds is known to be in $[0, 2^{64} - 1]$ and `i` in $[-2^{63}, 2^{63} - 1]$, so a contract that needs that range applies to them. The same holds for a record's fields, and for `U64` and `CHAR`.

An element of a `LIST` or `SEQ` of a number type keeps its range too. `xs.head`, `xs.tail.head`, `LIST_OPS.at(xs, i)`, an `x` with `xs.has(x)`, a fold's argument and the result of a generic routine such as `LIST_OPS.drop(xs, n)` are all known to be in range. So is a character of a line of a text, a `LIST[LIST[CHAR]]`. A byte read from `b.model` of a `BYTES` is also below 256, from the laws `model_octets`, `octets_head`, `octets_tail` and `octets_at`. A plain `LIST[U64]` is only in the range of `U64`, so to know that its elements are bytes, check them, as `if b < 256 then`.

The range is checked where a list is built. A routine that returns a `LIST[U64]`, an argument passed as one, and a field of that type must each have their elements in range, so `LIST.singleton(n)` with `n: MATH_INT` is refused there, as a `Range Violation`. A lambda keeps its result type's range, so `s.map(fn(x: U64): U64 do x *% 2 end)` is still a list of `U64`s.

A number is checked where it is made, as a list is. A routine that returns a `U64`, an argument passed as one, and a field of that type must each be in `U64`'s range. So `big: U64 do 18446744073709551616 end` is refused, and so is `take(m)` with `m: MATH_INT` and `take(x: U64)`, unless a `require` such as `m >= 0 and m < 100` bounds `m`. Both are a `Range Violation`. A parameter, a field, or a call of a routine declared to return a `U64`, is not checked again. A fold keeps its accumulator's range: `s.fold_left(fn(acc: U64, x: U64): U64 do acc *% 10 +% x end, 0)` is a `U64`, and so is a `MAP`'s `fold` whose start and steps are. The length of a list, `xs.length`, is a `NAT`. The verifier trusts this, since no list in memory holds $2^{64}$ elements.

A container is checked the same way. An `OPTION[U64]`, a `TUPLE[U64, BOOL]`, a `RESULT[STRING, U64]`, a `MAP[STRING, U64]` and your own `BOX[U64]` hold only `U64`s. So `Some(m)` with `m: MATH_INT`, returned as an `OPTION[U64]`, is a `Range Violation`, and a number read back out is in range: `o.get` of an `OPTION[NAT]` is a `NAT`, and so are `p.first` and the `x` of `case Some(x)`. One case is not checked where the value goes in: `m.put("a", x)` with `x: MATH_INT`, on a `MAP[STRING, NAT]`. Its result holds a `MATH_INT`, so a `NAT` read out of it is refused where it is used.

A function value keeps its result type's range too. Given `lex: (start: NAT, line: LIST[CHAR]) -> NAT`, `lex(s, line)` is a `NAT` when `s` and `line` are in range, so it can be the next state passed to `lex`. A function is checked where it is passed: a `(x: NAT) -> MATH_INT` given where a `(x: NAT) -> NAT` is expected is refused, as a `Range Violation`.

Two arms of one `match` may not bind the same name at different types, as `case JInt(v)` with `v: I64` beside `case JString(v)` with `v: LIST[U64]`. The verifier cannot translate it and reports `E0199`. Give each its own name.

### Refinement-Aware Pattern Matching

Pattern matching in Gardel is **proof-aware**:
1. **Exhaustiveness**: All reachable cases must be covered.
2. **Dead case elimination**: If the precondition or active proof context proves a case is mathematically impossible, you **must omit** that case. Including an impossible branch is rejected by the compiler!

```eiffel
-- Notice: No 'case None' needed!
-- The precondition 'Current.is_some' proves 'None' is unreachable.
get: A
  require
    present: Current.is_some
  do
    match Current
      case Some(v) then v
    end
  end
```

---

## 6. Contracts: Hoare Triples in Practice

A contract is the $P$ and $Q$ of a routine's Hoare triple: a mathematical agreement between callers and the routine they call. The keywords follow Eiffel, whose Design by Contract brought this idea into a programming language.

### Preconditions: `require`

A `require` block lists conditions the caller must establish before invoking the routine. If the caller cannot prove the precondition statically, the compiler refuses to build the calling code:

```eiffel
safe_divide(num: NAT, den: NAT): NAT
  require
    non_zero_divisor: den > 0
  do
    num / den
  end
```

### Postconditions: `ensure`

An `ensure` block lists conditions the routine guarantees upon return. The body must establish these conditions under the assumption that the `require` held:

```eiffel
increment(x: NAT): NAT
  require
    not_max: x < 18446744073709551615
  do
    x + 1
  ensure
    strictly_greater: Result > x
    exact_step: Result = x + 1
  end
```

### The Roles of `Current` and `Result`

- **`Current`**: The immutable receiver object on which a feature query or transformer is invoked (analogous to `self` or `this`).
- **`Result`**: The return value of the routine inside `ensure` clauses. In Gardel, functions are expressions, so the return value is the result of the `do` block.

### Tagged Predicates and AI Traceability

Every contract clause in Gardel begins with a human- and machine-readable tag:

```eiffel
require
  positive_balance: balance > 0
  under_ceiling: balance <= ceiling
```

Tags serve three vital purposes:
1. **Readable documentation**: Functions read like mathematical specifications.
2. **Pinpoint solver errors**: When a proof fails, the compiler names the exact failing tag (e.g., `VC failed: positive_balance`).
3. **Structured AI diagnostics**: AI agents receive the exact failed tag in `--json-diagnostics`, allowing targeted automated repair.

---

## 7. Modules, Visibility, and Verification Firewalls

### Module Structure (`representation`, `create`, `feature`, `specify`)

A standard Gardel `module` defines one Abstract Data Type organized into four distinct sections:

```eiffel
module BANK_ACCOUNT

-- 1. Internal representation: sum or product
representation {NONE}
  balance_cents: NAT

-- 2. Constructors: instantiate new values
create {ANY}
  open(initial_deposit: NAT): BANK_ACCOUNT
    do
      { balance_cents = initial_deposit }
    ensure
      initial_balance: Result.balance = initial_deposit
    end

-- 3. Runtime queries and transformers
feature {ANY}
  balance: NAT
    do
      Current.balance_cents
    end

  deposit(amount: NAT): BANK_ACCOUNT
    require
      no_overflow: Current.balance + amount <= 18446744073709551615
    do
      { balance_cents = Current.balance_cents + amount }
    ensure
      increased: Result.balance = Current.balance + amount
    end

-- 4. Ghost queries and lemmas (erased at compile-time)
specify {ANY}
  in_dollars: MATH_INT
    do
      Current.balance / 100
    end

-- 5. Module-wide Invariant (holds for ALL instances everywhere)
invariant
  solvency: balance_cents >= 0

end
```

### The 5 Visibility Rules

To prevent information leaks, circular dependencies, and unsound proofs across module boundaries:

1. **Contract Visibility**: Every symbol in a routine's `require` clause must be at least as visible as the routine itself. Callers cannot be asked to satisfy conditions they cannot see. The same holds for the `require` of an exported law: keep a law with a private hypothesis in `specify {NONE}`.
2. **Visible Ensures**: A client only sees `ensure` clauses containing symbols it has permission to see.
3. **Ghost Stays Ghost**: Symbols in `specify` can be used in contracts (`require`, `ensure`, `invariant`, `check`), but **never** in executable runtime code.
4. **Runtime-Checkable Preconditions**: Public preconditions should be expressible using public runtime queries so callers can write `if` statements before calling.
5. **Invariant Visibility**: Every symbol in an `invariant` must be visible to all modules permitted to inspect `representation`.

### Uniform Access Principle

In Gardel, client syntax does not distinguish between a stored field and a computed parameterless query:

```eiffel
-- Whether 'count' is a stored field or an algorithmically computed query:
let n = stack.count
```

This decouples the external API contract from internal memory representations.

---

## 8. Invariants as Refinement Types

### Class Invariants: Invariants Are Types

In traditional object-oriented languages, an object can temporarily violate its invariant while a method runs.

In Gardel, **an invariant is a type refinement**:
$$\text{Type } M \equiv \{ x \in M_{\text{raw}} \mid \text{Invariant}(x) \}$$

Because values are immutable:
- An instance of $M$ **never** violates its invariant.
- Once created, a value of type $M$ can be passed anywhere, stored in lists, or split across parallel threads without ever needing to re-prove its invariant.

### Construction-Site Verification

The verifier checks a module's invariant at every **construction site**:
- Every record literal `{ ... }`
- Every functional update `{ x with ... }`
- Every variant constructor `Some(v)`

If the active proof context cannot prove that the newly created value satisfies all `invariant` clauses, compilation fails immediately at that literal.

```eiffel
module PERCENTAGE

representation {NONE}
  value: NAT

create {ANY}
  from_nat(n: NAT): OPTION[PERCENTAGE]
    do
      if n <= 100 then
        -- Construction site: verifier proves value <= 100 here!
        Some({ value = n })
      else
        None
      end
    end

feature {ANY}
  as_nat: NAT
    do
      Current.value
    end

invariant
  in_range: value <= 100

end
```

### Hereditary Invariants and Destructuring

When pattern matching on recursive data structures (e.g. trees), subterms (like left and right subtrees) inherit the module's invariant automatically if the invariant is **hereditary** (inductive over subterms). For global non-hereditary properties (such as balanced AVL tree height or sorted binary search invariants), ghost models in `specify` provide clean, inductive representations.

---

## 9. Total Recursion and Termination Metrics

### The Totality Requirement (No Infinite Loops)

Gardel has no `while`, `for`, or `loop` keywords. All repetition is expressed through **structural recursion** or **tail recursion**.

To guarantee that programs never hang or crash:
> **Every recursive routine must prove termination at compile-time via a `variant` clause.**

The verifier builds the module's **call graph** and finds its recursive groups: a routine that calls itself, or routines that call each other in a cycle. At every call inside a group:
- both the caller and the callee must declare a `variant`, otherwise the call is rejected with `variant_missing`;
- the callee's variant, evaluated at the call's arguments, must be strictly smaller than the caller's current variant (`variant_decrease`), and an integer variant must stay non-negative (`variant_non_negative`).

A recursive routine without a `variant` never verifies, whatever its `ensure` claims. Without this rule, `f(n) do f(n) end` could "prove" any postcondition through its own non-terminating definition.

### Structural Induction with `variant`

When traversing inductive ADTs (like lists or trees), declare `variant Current` or `variant parameter_name`. The verifier proves that each recursive call operates on a strictly smaller subterm:

```eiffel
length[A](xs: LIST[A]): NAT
  variant
    xs
  do
    match xs
      case Empty      then 0
      case Cons(_, t) then 1 + length(t)
    end
  end
```

### Well-Founded Arithmetic Metrics

When recursing over numbers or ranges, provide an integer metric that strictly decreases toward 0 on every recursive step:

```eiffel
binary_search_range(lo: NAT, hi: NAT, target: NAT): BOOL
  require
    ordered: lo <= hi
  variant
    hi - lo
  do
    if lo = hi then
      False
    else
      let mid = lo + (hi - lo) / 2
      -- Both recursive calls decrease (hi - lo)
      if target < mid then
        binary_search_range(lo, mid, target)
      else
        binary_search_range(mid + 1, hi, target)
      end
    end
  end
```

Tail-recursive calls are automatically lowered by `gardelc` into **zero-allocation native C `while` loops**. Only self tail calls are loops; mutual recursion uses the stack.

### Lexicographic Variants

A `variant` may list several metrics. They are compared lexicographically: a call is fine when the first metric decreases, or the first stays equal and the second decreases, and so on.

```eiffel
-- Empties n, then refills it from m: n may grow (to 100), but only while m shrinks.
drain(m: NAT, n: NAT): NAT
  variant
    m, n
  do
    if n > 0 then
      drain(m, n - 1)        -- m equal, n smaller
    else
      if m > 0 then
        drain(m - 1, 100)    -- m smaller (n may grow)
      else
        0
      end
    end
  ensure
    empty: Result = 0
  end
```

Writing `drain(m, 100)` in the refill branch is rejected with `variant_decrease`: neither metric goes down.

A structural variant (`variant Current`, or a parameter of the module's own type) is measured by the size of the value, so recursing into a subterm decreases it.

### Mutual Recursion

Routines that call each other form one recursive group. Each must declare a `variant`, and every call between them must decrease it:

```eiffel
functions EVENODD

feature {ANY}

  is_even(n: NAT): BOOL
    variant
      n
    do
      if n = 0 then True else is_odd(n - 1) end
    end

  is_odd(n: NAT): BOOL
    variant
      n
    do
      if n = 0 then False else is_even(n - 1) end
    end

end
```

Replacing `is_even(n - 1)` with `is_even(n)` is rejected with `variant_decrease` in `is_odd`. Dropping either `variant` is rejected with `variant_missing`.

### No Recursion Through Function Values

A variant can only be checked at a **direct call**, where the arguments are known. A call hidden inside a lambda, or a routine passed around as a function value, may be applied anywhere and at any argument. So inside its own recursive group a routine may only be reached by direct calls:

```eiffel
-- REJECTED (E0315: higher_order_recursion): `f` is passed as a value inside its own group
f(n: NAT): NAT
  variant
    n
  do
    apply(f, n)
  end

-- REJECTED as well: the recursive call is inside a lambda
g(n: NAT): NAT
  variant
    n
  do
    apply(fn(x: NAT): NAT do g(x) end, n)
  end
```

Passing *non-recursive* routines and lambdas to higher-order routines (`map`, `filter`, `apply`, ...) is unrestricted.

### Loops That Run Fast in C

A routine over a long input, such as a parser over a document of a million bytes, runs as fast as a hand-written C loop if it follows two rules. `gjson`'s reader follows them.

- **Make the recursive call the last thing the routine does.** A self tail call is a `while` loop. Any other call takes a stack frame, and a 1 MB stack holds only 30,000 to 60,000 of them. Carry the result in an argument (an accumulator), and reverse it once at the end if needed.
- **Parallel work goes in divide-and-conquer routines, not in loops.** Two `let`s that do not use each other run as parallel OpenMP tasks when both may do unbounded work (each calls a recursive routine, directly or through other routines, applies a function value, or does IO) and the routine is not a loop (§10). A routine that calls itself only in tail position is a loop, and its `let`s always run in order, since a task on every iteration costs far more than most steps. So a loop's independent `let`s cost nothing extra, and a loop whose step does two heavy calls, such as summing two large `SEQ`s, runs them one after the other: split the work in halves instead (Task F.14 of the [libraries plan](../plan/libraries/verified_libraries.md#task-f14-a-cost-model-for-parallel-lets)).

`LIST.length` takes constant time in C, as each cell keeps the length of the list it starts, so a loop may test `bs.length` on every step.

---

## 10. Automatic Deterministic Parallelism

### Divide-and-Conquer Parallel Fork-Join

Gardel programs achieve massive concurrency **without threads, mutexes, locks, or channels**.

Parallelism is extracted automatically from **independent recursive bindings**:

```eiffel
let left_max  = l.max_value
let right_max = r.max_value
if left_max >= right_max then left_max else right_max end
```

Because `left_max` and `right_max` do not depend on one another and are purely functional, `gardelc` automatically compiles this into a parallel fork:
```c
// Emitted C11, simplified:
#pragma omp task shared(left_max)
left_max = max_value(l);
right_max = max_value(r);
#pragma omp taskwait
```

Both branches run in parallel on separate CPU cores, and task depth is capped so that small subtrees do not pay for a fork. The cap does not look at what each `let` costs, so two cheap `let`s in a loop still fork on every step: see [Loops That Run Fast in C](#loops-that-run-fast-in-c).

### Church-Rosser Confluence

Because functions have no side effects, the order in which parallel tasks finish has zero impact on the final result:
$$\text{Sequential Evaluation} \equiv \text{Parallel Execution}$$

Data races are mathematically impossible by design.

### The `SEQ[A]` Balanced Tree Collection

While `LIST[A]` is sequential (singly linked), the standard library collection `SEQ[A]` is represented as a balanced binary tree:

```eiffel
module SEQ[A]

representation {ANY}
  | Empty
  | Leaf(value: A)
  | Concat(left: SEQ[A], right: SEQ[A], len: NAT)
```

Operations like `map`, `reduce`, and `filter` over `SEQ[A]` split work between the two halves of the tree, which run as parallel tasks.

### Backends Today

| Output | Status |
| :--- | :--- |
| Boogie (`.bpl`) | The only verifier. Z3 discharges the obligations, bounded by solver work rather than time, so machine load cannot change a verdict. |
| C11 with OpenMP (`.c`) | Runs natively, and compiled by `gcc` when the input contains a `program`. |
| Bend v2 (`.bend`) | Written on each run, and the intended route to CUDA and Apple Metal GPUs. No emitted file has been executed yet, so treat GPU execution as a goal. |

There is no tracing garbage collector. Values are immutable and acyclic, so atomic reference counting frees everything, ownership moves on last use, and a thread-local allocator serves small blocks. Reusing a unique value in place is deferred.

---

## 11. Higher-Order Functions and Closures

### Function Signatures and Refined Types

Functions are first-class values. You can pass them as arguments, store them, or return them:

```eiffel
-- Total function from A to B:
A -> B

-- Refined function type with contracts:
(x: NAT) -> NAT require small: x < 1000 ensure bounded: Result <= x
```

The clauses of a refined type are checked like a routine's contract. On a parameter, the body may apply `f` only within its `require`, and it may use its `ensure`:

```eiffel
apply_small(f: (x: NAT) -> NAT require small: x < 100 ensure bounded: Result <= x, k: NAT): NAT
  require
    k_small: k < 100
  do
    f(k)            -- proves `small` for k
  ensure
    le: Result <= k -- follows from `bounded`
  end
```

A caller proves the `ensure` of what it passes. `apply_small(fn(x: NAT): NAT do x / 2 end, 50)` verifies. `apply_small(fn(x: NAT): NAT do x +% 1 end, 5)` is refused with `f_bounded`: each `ensure` becomes a `require` of the routine, named after the parameter and the clause.

What each position means:

- **A parameter** may have both clauses. Its `ensure` is proven by every caller, and its `require` at every application in the body.
- **A record field** may have an `ensure`, which becomes an invariant of the record (`handler_le` for a field `handler` and a clause `le`). It is proven where a record is built, so a `make` must take the function at the refined type, not a plain `NAT -> NAT`. In return, `r.handler(x)` has the contract wherever `r` comes from, an element of a list included.
- **A routine's result** may have an `ensure`, which becomes an `ensure` of the routine (`result_le`).
- **Anywhere else**, a `let`, a lambda's parameter, a `LIST[...]` or a function type inside another, clauses are `E0321`, as nothing would check them there.

A parameter with a `require` must not reach code that could apply it outside the `require`. It may be applied, or passed to a parameter whose type's `require` implies its own. `let g = f`, returning `f` or storing it is `E0322`. Passing it to `LIST_OPS.map`, whose `A -> B` has no `require`, or to `Some`, is refused by the verifier.

### Lambda Syntax (`fn`)

Anonymous functions use the `fn` keyword:

```eiffel
let add_one = fn(x: NAT): NAT do x + 1 end
```

A lambda can be applied to any value of its parameter types, so its body is proved for all of them, using what is known where it is created. This one is rejected, because `x + 1` overflows at `NAT.max`. A captured fact counts: `fn(y: U64): U64 do y / z end` verifies where `z > 0` is known, and is rejected with `div_by_zero` otherwise. A lambda passed straight to a parameter of a refined function type may also use that type's `require`: `two(fn(a: NAT, b: NAT): NAT do b - a end)` verifies when `two`'s parameter requires `a <= b`.

### A Routine as a Function Value

The name of a routine of the same module, with no call, is that routine as a function: `apply_small(halve, 50)` means `apply_small(fn(x: NAT): NAT do halve(x) end, 50)`, and it is checked exactly as that lambda is. `halve`'s `ensure` gives the `ensure` a refined type asks for, and its `require` is proven inside the lambda, where the type's `require` is known. So a routine whose `require` is weaker than the type's passes, and one whose `require` is stronger is refused with its own clause's name.

```eiffel
halve(x: NAT): NAT
  do
    x / 2
  ensure
    le: Result <= x
  end

by_name: NAT
  do
    apply_small(halve, 50)    -- verifies: `le` gives `bounded`
  end
```

A local, a parameter or a binder of the same name hides the routine. A generic routine is not turned into a function, as its lambda would need type arguments, and nor is a routine of another module: wrap it in a lambda. In a `module`, a lambda cannot call a sibling routine yet, as it does not capture `Current`, so this works in a `functions`, `program` or `reactive program` module.

### Compile-Time Closure Conversion (Lambda Lifting)

In many functional runtimes, closures capturing free variables require garbage collection or complex reference counting.

`gardelc` uses **compile-time lambda lifting**:
- Any lambda capturing variables is transformed into an explicit first-order record struct (the environment) paired with a top-level pure combinator.
- Environments are standard immutable ADTs that can be freely duplicated and dispatched across parallel threads without affine restrictions or heap leaks.

> [!IMPORTANT]
> Termination is checked only through direct calls: a routine may not reach its own recursive group through a lambda or a function value (`E0315`, see §9).

---

## 12. Concepts, Laws, and Mathematical Conformance

Gardel avoids implementation inheritance and subtyping. Instead, it relies on **Concepts** with verifiable algebraic laws.

### Declaring a `concept` and Laws

A concept specifies required method signatures and **algebraic laws** that conforming types must satisfy:

```eiffel
concept MONOID

  empty: Current
  combine(other: Current): Current

  law associativity(a: Current, b: Current, c: Current)
    ensure
      assoc: a.combine(b.combine(c)) = a.combine(b).combine(c)
    end

  law left_identity(a: Current)
    ensure
      left_id: empty.combine(a) = a
    end

end
```

### Proving Laws in `specify`

A module declares that it satisfies a concept with `satisfies CONCEPT`. It then **proves each law** inside a `specify {CONCEPT}` block:

```eiffel
module ADD_NAT satisfies MONOID

representation {ANY}
  val: NAT

create {ANY}
  empty: ADD_NAT
    do { val = 0 } end

feature {ANY}
  combine(other: ADD_NAT): ADD_NAT
    do { val = Current.val + other.val } end

specify {MONOID}

  law associativity(a: ADD_NAT, b: ADD_NAT, c: ADD_NAT)
    do
      -- Trivial for arithmetic addition: Z3 proves it automatically
    end

  law left_identity(a: ADD_NAT)
    do
      -- 0 + a = a: verified by SMT
    end

end
```

### Inductive Law Proofs

A law with a `do` body is proved by **structural induction**. A recursive call to the law inside its own body is the induction hypothesis. Like any recursive call it must respect the law's `require` clauses and decrease its `variant`, and the verifier then assumes the law's conclusion for those smaller arguments:

```eiffel
specify {EQ}

  law reflexive(a: PEANO)
    variant
      a
    do
      match a
        case Zero then True
        case Succ(p) then reflexive(p)   -- induction hypothesis: p.equals(p)
      end
    ensure
      refl: a.equals(a)
    end
```

### The Equality Hierarchy: `=`, `!=`, `equals`, and Models

Gardel distinguishes three kinds of equality:

| Form | Meaning | Allowed on |
| :--- | :--- | :--- |
| `a = b` | Structural (Leibniz) identity | Primitives except `F64`, ghost models such as `LIST[A]`, and types satisfying `CANONICAL_EQ` |
| `a.equals(b)` | Behavioral equivalence (the `EQ` concept) | Every `EQ` type, including non-canonical ones such as `SEQ[A]`, `BIGNAT`, `STRING` |
| `a.model = b.model` | Equality of canonical ghost models | Non-canonical runtime types with a ghost model (`SEQ.model`, `STRING.chars`) |

- **Why the split:** a balanced tree or a string buffer can represent the same value in several physical shapes. Using `=` on such a type would make the solver call equal values different, so it is rejected (for `STRING`: `E0311`).
- **`CANONICAL_EQ`** refines `EQ` with the law `a.equals(b) = (a = b)`: behavioral equality *is* structural identity.
- **`a != b`** is shorthand for `not (a = b)` and follows exactly the same rules as `=`.
- **Equality on a type parameter** uses the element type's own equality: for `[A: EQ]`, `x = y` on two `A` values means `A`'s equivalence, with reflexivity, symmetry and transitivity available as axioms.
- Parenthesize a negated comparison operand: write `Result = (not (a = b))`, not `Result = not (a = b)`.

### Total Orders (`ORD`)

The `ORD` concept refines `EQ` and requires irreflexivity, transitivity, and totality:

```eiffel
concept ORD refines EQ

  less_than(other: Current): BOOL

  less_or_equal(other: Current): BOOL
    do
      Current.less_than(other) or Current.equals(other)
    ensure
      definition: Result = (Current.less_than(other) or Current.equals(other))
    end

  law irreflexive(a: Current)
    ensure
      not_less_than_self: not a.less_than(a)
    end

  law transitive(a: Current, b: Current, c: Current)
    require
      a_lt_b: a.less_than(b)
      b_lt_c: b.less_than(c)
    ensure
      a_lt_c: a.less_than(c)
    end

  law total(a: Current, b: Current)
    ensure
      comparable: a.less_than(b) or a.equals(b) or b.less_than(a)
    end

end
```

This is the prelude's own `stdlib/concepts/ord.gdl`; `EQ`, `CANONICAL_EQ` and `ORD` are available everywhere without declaring them.

Generic modules like `SORTED_LIST[A: ORD]` can safely assume these laws as axioms during sorting and lookup proofs.

### Enumerations Get Their Instance

A sum whose variants have no fields needs no `equals` or `less_than` of its own:

```gardel
module LOG_LEVEL satisfies ORD

representation {ANY}
  | Debug
  | Info
  | Warn
  | Error

end
```

The compiler writes `equals` as `Current = other`, which also makes `LOG_LEVEL` `CANONICAL_EQ`, and `less_than` by the order the variants are written in. It adds:

- `t.ordinal`: 0 for `Debug`, up to 3 for `Error`.
- `LOG_LEVEL.from_ordinal(k)`: `Some(Warn)` for 2, `None` from 4 on.
- `LOG_LEVEL.all`: the four variants in order.
- The laws `ordinal_back`, `all_listed` and `all_at_ordinal`, which a client calls like any law: `LOG_LEVEL.all_listed(t)` gives `LOG_LEVEL.all.has(t)`.
- Every law of `EQ`, `CANONICAL_EQ` and `ORD` not written in the module.

All of it is verified, as if you had written it. A routine you write yourself is kept instead, and the laws are proven for yours. A sum with a field, or a record, must write `equals` and `less_than` (`E0323`).

### Bounds on Type Parameters

A bound gives a type parameter the methods of its concept, and the proof their laws:

```gardel
module SORTED[K: ORD]

representation {ANY}
  item: K

create {ANY}

  smaller(a: K, b: K): K
    do
      if a.less_than(b) then a else b end
    ensure
      least: not b.less_than(Result) and not a.less_than(Result)
    end

end
```

`K` is `EQ`, `CANONICAL_EQ`, `ORD` or a concept of your own. `a = b` on a `K` needs `CANONICAL_EQ`, because `equals` is not the identity under `EQ` or `ORD`.

Your own concept works the same way. Its methods become the parameter's, and its laws are what the proof knows of them:

```gardel
concept HASH refines EQ

  hash: U64

  law consistent(a: Current, b: Current)
    require
      ab: a.equals(b)
    ensure
      same: a.hash = b.hash
    end

end

module BAG[K: HASH]

representation {ANY}
  item: K

create {ANY}

  bucket(k: K, n: NAT): NAT
    require
      pos: n > 0
    do
      k.hash % n          -- `k.hash` is a U64, so it is below 2^64
    ensure
      below: Result < n
    end

  same_bucket(a: K, b: K, n: NAT): BOOL
    require
      pos: n > 0
      eq: a.equals(b)
    do
      True
    ensure
      claim: BAG.bucket(a, n) = BAG.bucket(b, n)   -- from `consistent`
    end

end
```

A concept with type parameters, or with a method that has a body, cannot be a bound (`E0320`): the proof would need that body for every instance.

The type is checked at every use, written or inferred. `SORTED.smaller(s, s)` on a `STRING` is `E0317`, because `STRING` is `EQ` and not `ORD`. So is `SORTED[F64]`. A method the bound does not give is `E0318`.

A client's call means the instance it chose, so `SORTED`'s contract is a fact about that type:

```gardel
nat_min(a: NAT, b: NAT): NAT
  do
    SORTED.smaller(a, b)
  ensure
    low: Result <= a and Result <= b     -- `less_than` on `NAT` is `<`
  end
```

Using a contract this way assumes the type keeps the laws of the bound, so it needs them proven elsewhere. A module that uses `SORTED` at its own type gets no such link: inside `LEVEL`, the contract of `SORTED.smaller(x, y)` says nothing about `LEVEL`'s order, or `LEVEL` could prove its own `ORD` laws with them.

> [!NOTE]
> A module's laws are axioms only for its **clients**. While the module itself is verified, its laws are proof obligations, never assumptions, so a law cannot be used to prove itself.

### Conditional Conformance (`where`)

A generic module can conform to a concept only when its type arguments do:

```eiffel
module SEQ[A] satisfies EQ where A satisfies CANONICAL_EQ
module RESULT[E, A] satisfies CANONICAL_EQ where E satisfies CANONICAL_EQ, A satisfies CANONICAL_EQ
```

The constraint is enforced where the concept's routines are used. `SEQ.equals` compares elements structurally, which is behavioral equality only for canonical elements, so `SEQ[U64].equals` is accepted while `SEQ[STRING].equals` and `SEQ[SEQ[U64]].equals` are rejected with `E0314: unsatisfied_where_constraint`. Primitives satisfy `CANONICAL_EQ` and `ORD`, a type parameter satisfies its declared bound (`[A: CANONICAL_EQ]`), and a module type satisfies the concepts it lists when its own `where` constraints hold.

A parameter can have both. In `module KEYED[K: ORD] satisfies HASH where K satisfies HASH`, every use of `KEYED` needs an `ORD` type, and inside `KEYED`, `K` has `less_than` and `hash` with the laws of both.

---

## 13. The Linear IO Firewall

### Why Pure Modules Forbid Effects

If functions could write to files or make network requests anywhere:
- Parallel execution would introduce race conditions.
- SMT solvers could not model mathematical invariants cleanly.
- Code generation could not safely reorder or optimize expressions.

Gardel establishes a strict **Purity Firewall**: standard modules are 100% pure. All side effects are confined to `program` modules.

### Linear `IO[A]` Actions and `with IO`

An `IO[A]` value is an unexecuted linear action token that must be consumed **exactly once**:

```eiffel
program ECHO

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        line_res <- IO.read_line
        match line_res
          case Ok(line) then
            with IO
              IO.print("You said: ")
              IO.print_line(line)
              IO.exit(0)
            end
          case Error(_) then
            IO.exit(1)
        end
      end
    end

end
```

Inside `with IO ... end`:
- `x <- e`: Monadic bind. Evaluates an IO action $e$ of type `IO[T]` and binds the result $x : T$.
- `e`: Statement step. Evaluates an action of type `IO[UNIT]` for its effect.
- The trailing expression must return `IO[R]`; it may be a `match` whose branches are `IO` actions or nested `with IO` blocks.

### Reading Input: `IO.read_line`

```eiffel
IO.read_line : IO[RESULT[IO_ERROR, STRING]]
```

Reading can fail, so the result is a `RESULT` that must be matched:
- `Ok(line)`: one line without its `\n` / `\r\n` terminator, of any length.
- `Error(EndOfInput)`: the input is exhausted.
- `Error(OsError(code))`: the operating system reported an error.
- `Error(Decode(cause))`: the line is not valid text (in the ASCII bootstrap: a byte `>= 128`), with a `STRING_ERROR` cause such as `InvalidByte(offset, byte_val)`.

For the verifier, every `<- IO.read_line` is a **fresh, unknown value**: two reads are never assumed equal. The only thing assumed is the boundary guarantee that an `Ok` payload is a valid `STRING`. The native C runtime establishes that guarantee by validating every byte before building the `STRING`.

### The Time and Random Bytes: `IO.now`, `IO.random`

```eiffel
IO.now : IO[I64]                                  -- milliseconds since 1970, UTC
IO.random(count: NAT) : IO[RESULT[IO_ERROR, BYTES]]  -- from the system's generator
```

Both are fresh values, as a line is. The verifier knows a time is an `I64` and nothing more: not that it is after 1970, and not that a second read is later than the first, since the system's clock can be set back. It knows an `Ok` of `IO.random(n)` holds exactly `n` bytes, so a routine that needs 16 can be given `IO.random(16)`'s bytes with no check, and two draws are never known to be equal. More than 1024 bytes in one call is an `Error`.

```eiffel
with IO
  r <- IO.random(16)
  match r
    case Ok(key) then IO.exit(serve(key))   -- serve's `require key.length = 16` holds
    case Error(_) then IO.exit(1)
  end
end
```

### Logging and the Environment: `IO.log`, `IO.env`

```eiffel
IO.log(level: LOG_LEVEL, text: STRING) : IO[UNIT]   -- one line on standard error
IO.env(name: STRING) : IO[OPTION[STRING]]           -- an environment variable
```

`IO.print_line` is the program's output; `IO.log` is for what its operator reads, and goes to standard error as one line, such as `warn: disk almost full`. `LOG_LEVEL` is `Debug | Info | Warn | Severe`, in that order (it is an ordered enumeration, so `Info.less_than(Warn)` holds). `GARDEL_LOG_LEVEL` names the lowest level written, `info` when it is not set, in any case; any other value stops the program with exit status 2 before it runs. A control byte or a `\` in the text is written escaped, as `\x0a` or `\\`, so a line from a request cannot end its line early or forge another.

`IO.env(name)` is a fresh value, as a line is: `None` when the variable is not set, or when its value is not UTF-8, and the verifier knows nothing more, not even that two reads agree. A variable set to nothing is `Some("")`. Windows matches names in any case, POSIX exactly.

```eiffel
with IO
  port <- IO.env("PORT")
  IO.log(Info, match port case Some(p) then "port ".concat(p) case None then "no port" end)
  IO.exit(0)
end
```

### Boundary Ingestion: Untrusted Data to Verified ADTs

External data (network packets, command line inputs, disk files) is inherently untrusted.

Untrusted data cannot be cast directly into an invariant-carrying ADT. It must pass through a **boundary constructor** returning `OPTION[T]` or `RESULT[E, T]` (error type first):

```eiffel
-- Safe boundary parser:
parse_port(raw_string: STRING): OPTION[PORT_NUMBER]
  do
    let n = string_to_nat(raw_string)
    if n > 0 and n <= 65535 then
      Some(PORT_NUMBER.make(n))
    else
      None
    end
  end
```

Once inside `Some(port)`, all downstream code relies on the static guarantees of `PORT_NUMBER` with zero runtime validation overhead!

---

## 14. Reactive Programs

A `program` runs and exits. A `reactive program` runs until told to stop, as a server, a terminal interface or a game does, and every routine in it is still total. Non-termination lives in one visible place, the runtime's event loop.

### Two Total Routines

```eiffel
reactive program REACTIVE_HELLO

feature {ANY}

  init(args: SEQ[STRING]): STEP[NAT]
    do
      Continue(0, LIST.empty.prepend(Print("Hello. Press Ctrl-C to stop.")))
    end

  step(s: NAT, e: EVENT): STEP[NAT]
    do
      match e
        case Shutdown then Stop(0, LIST.empty.prepend(Print("Goodbye.")))
        case _ then Continue(s, LIST.empty)
      end
    end

end
```

The runtime calls `init`, performs the commands it returns, waits for an event, calls `step`, and repeats until a step returns `Stop`. Routines describe commands as values and the host performs them:

| Prelude type | Contents |
| :--- | :--- |
| `STEP[S]` | `Continue(next, cmds)` or `Stop(code, cmds)` |
| `COMMAND` | `Print`, `Write`, `TerminalSize`, `SetTimer`, `CancelTimer`, `ReadLine`, `ReadKey`, `Listen`, `Connect`, `Send`, `Close`, `ReadFile`, `ListDir`, `RunProcess`, `WriteProcess`, `CloseInput`, `StopProcess`, `RunAttached`, `WriteFile`, `ReadClock`, `Random`, `Log`, `ReadEnv` |
| `EVENT` | `Tick`, `Line`, `Key`, `Resized`, `Accepted`, `ListenFailed`, `Connected`, `ConnectFailed`, `Received`, `SendFailed`, `Closed`, `FileRead`, `DirListed`, `ProcessStarted`, `ProcessFailed`, `ProcessOutput`, `ProcessErrorOutput`, `ProcessExited`, `AttachedEnded`, `FileWritten`, `Pasted`, `Clock`, `RandomBytes`, `EnvRead`, `Shutdown` |
| `KEY` | `Char(c)`, arrows, `Enter`, `Escape`, `Backspace`, `Tab`, `Home`, `End`, `PageUp`, `PageDown`, `Delete`, `Insert`, `Function(n)`, `Ctrl(c)`, `Mod(key, shift, alt, ctrl)`, `Other`, `KeysEnded` |
| `CONN_ID`, `PROC_ID` | Opaque connection and process handles: they can be compared and stored, never forged |
| `DIR_ENTRY` | An entry of a listing: `name` and `is_dir` |
| `LOG_LEVEL` | `Debug`, `Info`, `Warn`, `Severe`, in that order |

Rules worth knowing:
- There is no `main`, no `IO` type and no `with IO`. `Stop` is the only way out, and its `code` is the exit status.
- `init` and `step` have no `require`, because the runtime calls them with every input.
- End a `match` on `EVENT` with `case _`.
- `Connect(tag, host, port)` is answered by `Connected(tag, conn)` or `ConnectFailed(tag, err)`, with the tag you chose. `host` is an IPv4 address or `localhost`: names are not looked up. A connection you made works like an accepted one.
- `CancelTimer(id)` clears a pending timer, so a stale `Tick` cannot reach a later use of the id.
- A command never blocks the loop. One that can fail reports it as a later event.
- To draw a screen, `Write` text with no newline, VT sequences included: a step's output goes out together, and the bytes are the same on Linux and Windows. `TerminalSize` answers with `Resized(width, height)`, and every later resize arrives the same way. Output that is not a terminal is 80 by 24. The `gtui` package draws a screen this way, writing only the cells that change: see [A Package for Terminal Interfaces](#a-package-for-terminal-interfaces-gtui).
- `ReadFile(tag, path)` answers with `FileRead(tag, res)`, the whole file as `BYTES`, and `ListDir(tag, path)` with `DirListed(tag, res)`, the entries sorted by name. Both run on a thread of their own.
- `WriteFile(tag, path, data)` answers with `FileWritten(tag, res)`, the number of bytes written. It writes a new file beside `path` and renames it over `path`, so a crash leaves the old file or the new one, never half of one. It replaces a file that is there: read it first if that must not happen.
- `ReadClock(tag)` answers with `Clock(tag, wall_millis, monotonic_millis)`. The wall clock is milliseconds since 1970 in UTC, for showing a date or stamping a record; it can jump when someone sets the system's clock. The monotonic clock is the one timers use: measure how long something took with it. The answer comes in the next step, as every answer does.
- `Random(tag, count)` answers with `RandomBytes(tag, res)`: `count` bytes from the system's generator, at most 1024, for keys, tokens and ids that must not be guessed. The bytes are written to a recording, since a replay needs them, so **a recording of a program that makes secrets is itself a secret**: keep it as you keep the keys. An `Ok` holds `count` bytes, but the event does not say what you asked for, so check the length where you take them in (`if bs.length = 16 then ...`).
- `Log(level, text)` writes one line on standard error, as `IO.log` does (above), at once and in order with what the step printed before it. It is logged and compared on a replay like `Print`, so a replay writes the same lines.
- `ReadEnv(tag, name)` answers with `EnvRead(tag, value)`, the variable's value as an `OPTION[STRING]`, in the next step. A server reads its port this way, in `init`, and listens once the answer comes: see `examples/todo_api`.
- `RunProcess(tag, program, args, dir)` starts a program, with no shell: each argument reaches it as you wrote it. It answers with `ProcessStarted(tag, proc)` or `ProcessFailed(tag, err)`. Then `ProcessOutput(proc, data)` and `ProcessErrorOutput(proc, data)` bring what it writes, and `ProcessExited(proc, code)` comes last. `WriteProcess(proc, data)` and `CloseInput(proc)` feed its input, and `StopProcess(proc)` kills it and whatever it started.
- `RunAttached(tag, program, args, dir)` gives a program the terminal, say an editor or another full-screen program, and answers with `AttachedEnded(tag, res)` when it ends. Your keys go to it meanwhile. After it, `Resized` comes again, so a screen is drawn anew.
- `Ctrl(c)` is Ctrl with a letter, given lowercase. Ctrl-H, Ctrl-I, Ctrl-J and Ctrl-M are `Backspace`, `Tab` and `Enter`, and Ctrl-C is always a `Shutdown`, so a program in raw mode can still be stopped and leaves the terminal as it found it.
- Other keys with Shift, Alt or Ctrl come as `Mod(key, shift, alt, ctrl)`: Shift-Tab is `Mod(Tab, True, False, False)`, Ctrl-Right `Mod(Right, False, False, True)`, Alt-x `Mod(Char('x'), False, True, False)`. A `match` with no `Mod` case takes them in its `case _`. F1 to F12 are `Function(n)`.
- A program that turns on bracketed paste (`gtui` does) gets a paste as one `Pasted(text)`, in answer to a `ReadKey`, so ask for the next key after it. Its text has LF line ends and no control character but Tab.
- The compiler adds `e.is_shutdown implies Result.is_stop` to every `step`, tagged `shutdown_must_stop`, and a clause you write cannot replace it.

### The State Proves Itself

Put the state in an opaque module with an `invariant`. Only that module can build a value, so the invariant holds after `init` and after every step, however long the program runs, with no new kind of proof:

```eiffel
module SERVER_STATE

representation {NONE}
  live: LIST[CONN_ID]
  limit: NAT

invariant
  within_limit: live.length <= limit

create {ANY}

  make(cap: NAT): SERVER_STATE
    do
      { live = LIST.empty, limit = cap }
    end

feature {ANY}

  has_room: BOOL
    do
      Current.live.length < Current.limit
    end

  admit(c: CONN_ID): SERVER_STATE
    require
      room: Current.has_room
    do
      { Current with live = Current.live.prepend(c) }
    end

end
```

`step` must test `s.has_room` before calling `s.admit(c)`, or its `room` precondition fails. See the complete programs in `examples/echo_server/`, `examples/reactive_clock/` and `examples/reactive_game/`. Give the folder to `gardelc`:

```bash
gardelc examples/reactive_game --strict
./build/reactive_game_bin
```

### What Is and Is Not Guaranteed

| Guaranteed, given the proofs | Not guaranteed |
| :--- | :--- |
| No crash, out-of-bounds access or overflow in any step | That events ever arrive |
| The state invariant after every step | That commands succeed, or how long they take |
| Each event is handled in finite time | The arrival order of concurrent events |
| A `Shutdown` ends the run with the code `step` chose | Liveness, or `Shutdown` on `SIGKILL` or power loss |

### Scripted Runs, Record and Replay

A reactive program is a pure function of its arguments and its events, so a run can be repeated exactly:

| Variable | Effect |
| :--- | :--- |
| `GARDEL_EVENTS=<script>` | Run from a script, such as `Resized:80:24 Tick:1 Key:Up Key:Ctrl:s Line:hello%20there Shutdown`. The end of the script is a `Shutdown`. On Linux one variable holds at most 128 KiB, so a longer run is a replay. |
| `GARDEL_RECORD=<file>` | Write a log of the run |
| `GARDEL_REPLAY=<file>` | Run from a log, with no timers, reads, sockets, files, processes, clock, generator or environment |
| `GARDEL_LISTEN_ADDR=<ip>` | Bind `Listen` to an address other than loopback |
| `GARDEL_CONNECT_TIMEOUT_MS=<n>` | How long a `Connect` may take before it fails, 10000 by default |
| `GARDEL_LOG_LEVEL=<level>` | The lowest level of `Log` and `IO.log` written: `debug`, `info` (the default), `warn` or `severe` |

The log holds the commands and the exit code as well as the events. A replay compares each command before it takes effect, and the first difference stops the program with exit status 3 and the line that differs, so a log from a working build is a regression test for a changed one. These are environment variables, not flags, because the command line belongs to the program and is part of what is replayed.

A log holds everything the program received, in the clear, and has no size limit: random bytes too, and the times read, so a replay on another day gives the same output, and a log of a program that makes keys from `Random` is as secret as the keys. In a script, `Clock:1:1795000000000:42` and `RandomBytes:2:%00%FF` give a time and two bytes, and `EnvRead:3:8080` and `EnvMissing:3` a variable's value and its absence. The value of a variable read is in the log too. A replay shows that the commands repeat, not that their real effects did.

---

## 15. SMT Proofs & AI Agent Verification Workflow

Gardel is specifically engineered for autonomous collaboration between human architects and AI coding agents.

### The Boogie / Z3 Verification Engine

```
 [ Gardel Source (.gdl) ]
           │
           ▼
 [ Typecheck & Contracts ]
           │
           ▼
 [ VC Gen: Boogie IR ]
           │
           ▼
   [ Z3 SMT Solver ]
      ├── PROVED! ───► [ Codegen: Bend v2 / C11 ]
      └── FAILED! ───► [ --json-diagnostics for AI Repair ]
```

Every function generates verification conditions (VCs):
1. **Preconditions of callees**: Caller must satisfy callee's `require`, for calls into the same module, other project files, and the Standard Prelude alike.
2. **Postconditions**: Routine body must establish its `ensure`.
3. **Invariants**: Every constructed value must satisfy its `invariant`.
4. **Numerics**: Arithmetic operations must not overflow or divide by zero.
5. **Termination**: Every call inside a recursive group must decrease the `variant` (§9).
6. **Concept laws**: Every law of every concept a module `satisfies` must be proved.

**One false contract never hides another failure.** A routine's verified contracts are used as facts when checking its callers. If a contract is false, that "fact" would be a contradiction from which anything follows, so it could hide every other failure that depends on it. `gardelc` therefore verifies to a fixpoint: each failed postcondition clause (or the whole contract of a routine that fails in another way) is withdrawn, and verification repeats until no new failure appears. Every reported failure is genuine, and none is masked.

### The AI Autonomous Repair Loop (`--json-diagnostics`)

When verification fails, run `gardelc --json-diagnostics` to receive structured, machine-actionable JSON:

```bash
gardelc stack.gdl --json-diagnostics
```

The output is a JSON array with one object per diagnostic (abridged here; the raw solver model in `counterexample` can be long):

```json
[
  {
    "level": "error",
    "rule": "Phase 3 Boogie (Postcondition Violation)",
    "module": "BOUNDED_STACK",
    "routine": "push",
    "tag": "count_incremented",
    "contract_tag": "count_incremented",
    "span": { "file": "stack.gdl", "line": 42, "column": 7, "end_line": 42, "end_column": 51 },
    "path_conditions": [],
    "counterexample": { "Current.size": "0", "Result.size": "0", "x": "7" },
    "relevant_lemmas": [],
    "message": "Formal verification condition `count_incremented` failed in `push`"
  }
]
```

`rule` names the kind of failure, `routine` and `tag` locate the exact clause, and `span` points at it in the source.

### Reading Counterexample Models

The counterexample gives the exact input state where the theorem fails:
- Here, `Current.size` was `0`, but `Result.size` remained `0` after push!
- The bug is immediately obvious: `{ Current with items = items.prepend(x) }` forgot to update `size = Current.size + 1`.

### Repair Patterns for LLMs

When repairing failing Gardel code:

| Diagnostic (`rule` / `tag`) | Common Root Cause | Recommended AI Fix |
| :--- | :--- | :--- |
| Arithmetic Overflow / `overflow_add`, `overflow_sub`, `overflow_mul` | Unchecked `a + b`, `a - b`, `a * b` | Add a `require` bound or rewrite (e.g. `lo + (hi - lo) / 2`) |
| Division by Zero / `div_by_zero` | Divisor not proven non-zero | Add `require d > 0` or branch on `d = 0` |
| Empty List / `.head`, `.tail` | `xs.head` or `xs.tail` of a list not proven non-empty | Add `require xs.length > 0`, or test it first: `if xs.length > 0 then`, `xs.length > 0 and` |
| Termination Variant Violation / `variant_decrease` | A recursive call does not shrink the `variant` | Check the call's arguments; pick a metric that decreases on every call in the group |
| Termination Variant Violation / `variant_missing` | A routine in a recursive group has no `variant` | Add a `variant` to every routine of the group |
| Termination Variant Violation / `variant_non_negative` | An integer metric can go below 0 | Strengthen `require` or choose a `NAT` metric |
| `E0315: higher_order_recursion` | Recursion through a lambda or function value | Call the routine directly |
| Invariant Violation / invariant tag | Constructed value breaks the `invariant` | Insert a `check` to prove the intermediate lemma, or add a guard `if` |
| Precondition Violation / callee's `require` tag | Callee requires a condition the caller does not establish | Wrap the call in `if precondition_test then ... else ... end` |
| Postcondition Violation / `ensure` tag | Result does not match the claim | Inspect counterexample values to spot omitted field updates |
| `E0314: unsatisfied_where_constraint` | `equals` on e.g. `SEQ[STRING]` | Compare models (`a.model = b.model`) or use canonical elements |
| `E0317: unsatisfied_bound` | `SORTED[STRING]` with `K: ORD`, written or inferred | Use a type that satisfies the bound, or weaken the bound |
| `E0318: method_not_in_bound` | `a.less_than(b)` on `K: EQ`, or `a.equals(b)` on a bare `K` | Add or strengthen the bound |
| `E0319: variant_no_measure` | `variant xs` on a `LIST` or a `STRING` | Use a number (`xs.length`), or a parameter of a sum type |
| `E0320: unsupported_bound` | `[K: C]` where `C` has type parameters or a method with a body | Move the body to each instance, or bound by a concept without one |
| `E0321: refined_function_type_position` | `let g: ((x: NAT) -> NAT ensure ...) = ...`, `LIST[(x: NAT) -> NAT ensure ...]`, a `require` on a field | Put the clauses on a parameter, a field or a result; a `require` on a parameter only |
| `E0322: refined_function_escapes` | `let g = f` or returning `f`, where `f`'s type has a `require` | Apply `f`, or pass it to a parameter whose `require` implies `f`'s |
| `E0323: underivable_instance` | `module CELL satisfies CANONICAL_EQ` with a variant `Full(n: NAT)` and no `equals` | Write `equals` (and `less_than` for `ORD`); only an enumeration gets them from the compiler |

---

## 16. Complete Real-World Examples

### Example A: `BOUNDED_STACK[A]`

A classic abstract data type with full invariants, capacity safety, and overflow-proof bounds:

```eiffel
module BOUNDED_STACK[A]

representation {NONE}
  items: LIST[A]
  size: NAT
  limit: NAT

create {ANY}

  make(cap: NAT): BOUNDED_STACK[A]
    require
      positive_capacity: cap > 0
    do
      { items = LIST.empty, size = 0, limit = cap }
    ensure
      is_empty: Result.count = 0
      capacity_set: Result.capacity = cap
    end

feature {ANY}

  count: NAT
    do
      Current.size
    end

  capacity: NAT
    do
      Current.limit
    end

  push(x: A): BOUNDED_STACK[A]
    require
      not_full: Current.count < Current.capacity
    do
      { items = Current.items.prepend(x),
        size = Current.size + 1,
        limit = Current.limit }
    ensure
      count_incremented: Result.count = Current.count + 1
      capacity_unchanged: Result.capacity = Current.capacity
    end

  pop: TUPLE[A, BOUNDED_STACK[A]]
    require
      not_empty: Current.count > 0
    do
      let rest = { items = Current.items.tail,
                   size = Current.size - 1,
                   limit = Current.limit }
      (Current.items.head, rest)
    ensure
      count_decremented: Result.second.count = Current.count - 1
    end

invariant
  positive_capacity: limit > 0
  size_matches: size = items.length
  within_bounds: size <= limit

end
```

---

### Example B: Verified Parallel Tree Construction & Maximum

Constructs a balanced binary tree over `[lo, hi]` in parallel and computes the maximum leaf value in parallel:

> [!NOTE]
> `examples/tree.gdl` currently reports 5 unproven postconditions (`max_value` and `from_range`): the verifier's `LIST` theory still lacks a few lemmas (e.g. about `has` on singleton and empty lists). These are genuine gaps in what can be proved yet, not hidden errors.

```eiffel
module TREE

representation {ANY}
  | Leaf(value: NAT)
  | Node(left: TREE, right: TREE)

specify {ANY}

  leaves: LIST[NAT]
    variant
      Current
    do
      match Current
        case Leaf(v)    then LIST.empty.prepend(v)
        case Node(l, r) then l.leaves.append(r.leaves)
      end
    end

create {ANY}

  from_range(lo: NAT, hi: NAT): TREE
    require
      valid_range: lo <= hi
    variant
      hi - lo
    do
      if lo = hi then
        Leaf(lo)
      else
        let mid = lo + (hi - lo) / 2
        -- Independent let bindings run in parallel:
        let left_tree = TREE.from_range(lo, mid)
        let right_tree = TREE.from_range(mid + 1, hi)
        Node(left_tree, right_tree)
      end
    ensure
      non_empty: Result.leaves.length > 0
      contains_lo: Result.leaves.has(lo)
      contains_hi: Result.leaves.has(hi)
    end

feature {ANY}

  max_value: NAT
    variant
      Current
    do
      match Current
        case Leaf(v) then
          v
        case Node(l, r) then
          -- Independent recursive calls run as parallel tasks:
          let ml = l.max_value
          let mr = r.max_value
          if ml >= mr then ml else mr end
      end
    ensure
      upper_bound: forall x: NAT. Current.leaves.has(x) implies x <= Result
      attained: Current.leaves.has(Result)
    end

end
```

---

### Example C: Peano Numbers with Formally Verified Total Ordering

Demonstrating algebraic concepts, total orders (`ORD`), and structural induction proofs. The concepts come from the prelude; the module proves every law of `EQ`, `CANONICAL_EQ` and `ORD` by induction (abridged from `examples/peano_ord.gdl`, which verifies with 0 errors):

```eiffel
module PEANO satisfies ORD, CANONICAL_EQ

representation {ANY}
  | Zero
  | Succ(pred: PEANO)

feature {ANY}

  equals(other: PEANO): BOOL
    variant
      Current
    do
      match Current
        case Zero then
          match other
            case Zero then True
            case Succ(_) then False
          end
        case Succ(p) then
          match other
            case Zero then False
            case Succ(q) then p.equals(q)
          end
      end
    ensure
      structural: Result = (Current = other)
    end

  less_than(other: PEANO): BOOL
    variant
      Current
    do
      match Current
        case Zero then
          match other
            case Zero then False
            case Succ(_) then True
          end
        case Succ(p) then
          match other
            case Zero then False
            case Succ(q) then p.less_than(q)
          end
      end
    end

specify {EQ}

  law reflexive(a: PEANO)
    variant
      a
    do
      match a
        case Zero then True
        case Succ(p) then reflexive(p)
      end
    ensure
      refl: a.equals(a)
    end

  -- symmetric and transitive follow the same pattern

specify {CANONICAL_EQ}

  law agrees_with_structural(a: PEANO, b: PEANO)
    variant
      a
    do
      match a
        case Zero then True
        case Succ(p) then
          match b
            case Zero then True
            case Succ(q) then agrees_with_structural(p, q)
          end
      end
    ensure
      sound: a.equals(b) = (a = b)
    end

specify {ORD}

  law irreflexive(a: PEANO)
    variant
      a
    do
      match a
        case Zero then True
        case Succ(p) then irreflexive(p)
      end
    ensure
      not_less_than_self: not a.less_than(a)
    end

  law total(a: PEANO, b: PEANO)
    variant
      a
    do
      match a
        case Zero then True
        case Succ(p) then
          match b
            case Zero then True
            case Succ(q) then total(p, q)
          end
      end
    ensure
      comparable: a.less_than(b) or a.equals(b) or b.less_than(a)
    end

  -- transitive follows the same pattern

end
```

Each recursive law call (`reflexive(p)`, `total(p, q)`, ...) is the induction hypothesis for the smaller argument (§12).

---

## 17. Projects and the Standard Prelude

### The Prelude Is Always There

Every program automatically sees the **Standard Prelude**:

| Kind | Names |
| :--- | :--- |
| Concepts | `EQ`, `CANONICAL_EQ`, `ORD` |
| Core types | `OPTION[A]`, `RESULT[E, A]`, `IO_ERROR` |
| Collections | `SEQ[A]`, `LIST[A]`, `LIST_OPS`, `ARRAY[A]`, `MAP[K, V]`, `SET[K]`, `QUEUE[A]`, `HEAP[K]`, `KEY_ORDER[K]`, `HEAP_ORDER[K]`, `SORTING[K]`, `GENERATOR[S, A]`, `UNFOLD[S, A]` |
| Numbers | `BIGNAT`, `NUMBER_TEXT`, `NUMBER_ERROR` |
| Text | `STRING`, `STRING_ERROR`, `BYTES`, `BYTES_BUILDER`, `STRING_BUILDER`, `TEXT_FORMAT` |
| Encodings | `HEX`, `BASE64`, `PERCENT`, `CODEC_ERROR` |
| Functions | `FUNCTION_OPS` |
| Reactive programs | `STEP[S]`, `COMMAND`, `EVENT`, `KEY`, `CONN_ID` |

The prelude is verified once, on its own (`gardelc --stdlib`). In user programs its contracts and laws are trusted facts, and calls into it must still establish its preconditions. You may declare a name the prelude already uses: the bare name then means yours, and `GARDEL::NAME` means the prelude's. Pass `--no-prelude` only to build a prelude-free program on purpose.

### Lists and Their Laws: `LIST_OPS`

`LIST_OPS` has the list operations of Bird and Wadler's *Introduction to Functional Programming*, §3.2 to §3.5. Each is total and runs in C:

| Operation | Gives |
| :--- | :--- |
| `map`, `filter`, `fold_left`, `fold_right`, `at(xs, i)` | As usual. `at` needs `i < xs.length` |
| `take(xs, n)`, `drop(xs, n)` | The first `n` items and the rest. With fewer than `n` items, `take` gives them all and `drop` none |
| `take_while(xs, p)`, `drop_while(xs, p)` | The longest prefix whose items satisfy `p`, and what follows it |
| `reverse(xs)`, `reverse_onto(xs, acc)` | The items in reverse order; the second puts them in front of `acc` |
| `concat(xss)`, `concat_map(xs, f)` | The lists one after another; `concat_map` concatenates the lists `f` gives |
| `zip(xs, ys)`, `zip_with(xs, ys, f)` | Pairs `(x, y)` or `f(x, y)` of items at the same place, as long as the shorter list |
| `last(xs)`, `init(xs)` | The last item, and every item but it. Both need `xs.length > 0` |
| `range(lo, hi)`, `iterate_n(f, x, n)` | `lo` up to but not including `hi`; and `x`, `f(x)`, `f(f(x))` and so on, `n` items |
| `update(xs, i, x)` | `xs` with `x` at `i`, or `xs` itself when `i` is past the end |

A contract states each one's length and how it unfolds once (`empty_case`, `cons_case`). What takes induction to prove is a law. Call the law where you need it:

```eiffel
  twice(xs: LIST[U64]): LIST[U64]
    do
      check
        back: LIST_OPS.reverse_reverse(xs)
      end
      LIST_OPS.reverse(LIST_OPS.reverse(xs))
    ensure
      same: Result = xs
    end
```

The laws:
- **Lists:** `at_update_same` and `at_update_other` (what `at` gives after an `update`; the solver applies them on its own), `take_drop` (`take(xs, n).append(drop(xs, n)) = xs`), `take_while_drop_while`, `reverse_reverse`, `reverse_append` (`reverse(xs.append(ys)) = reverse(ys).append(reverse(xs))`), `reverse_cons`, `init_last`, `map_fusion` (`map(map(xs, g), f) = map(xs, FUNCTION_OPS.compose(f, g))`), `filter_map`, `map_append` and `filter_append`.
- **Folds:**
  - `fold_duality_first`: over a monoid, `fold_right` and `fold_left` agree.
  - `fold_duality_second`: the same for two functions that associate with each other.
  - `fold_duality_third`: `fold_right(xs, f, a) = fold_left(reverse(xs), g, a)`, where `g` is `f` with its arguments swapped.
  - `fold_homomorphism`: a monoid fold of `xs.append(ys)` combines the folds of each.
- **`concat_map`:** it is `concat` after `map` (`concat_map_is_concat_map`). It also has the monad laws, named as `OPTION.and_then`'s are: `concat_map_left_unit`, `concat_map_right_identity` and `concat_map_associative`.

A law about a function needs something of that function. It asks for it as `forall` premises in its `require`, and your call must prove them for the function you pass. So `fold_duality_first(xs, f, e)` needs `f` to be associative with `e` as its identity on both sides. A function you write as `fn(p: BOOL, q: BOOL): BOOL do p or q end` meets that. A law takes the function another law would build, such as `g` for the third theorem or `h` for associativity, as a parameter, with a premise saying what it is: a law that holds an `fn` is not given to clients.

Two limits:
- A premise over a number type quantifies over every integer, not only those of the type, so a function on `NAT` must meet it for negative numbers too. A function on `BOOL`, or one that never does arithmetic, meets it most easily.
- `take`, `map` and the other operations recurse once per item, so a list of millions of items can exhaust the C stack.

`SEQ.reduce(f, e)` folds the two sides of each `Concat` apart, so they may run in parallel. The law `SEQ.reduce_folds(s, f, e)` has the same monoid premises as `fold_duality_first`. It states that `reduce` then equals both `fold_left` and `fold_right` of the model, proven by `fold_homomorphism` and the first duality theorem.

### Sorting: `SORTING`

`SORTING[K: ORD]` sorts a `LIST[K]` of any ordered type: numbers, `STRING`, `BIGNAT`, an enumeration, or a module of yours that satisfies `ORD`.

| Routine | Gives |
| :--- | :--- |
| `SORTING.sort(xs)` | The items of `xs` in order, by merge sort |
| `SORTING.merge(xs, ys)` | The items of both, in order when both are |
| `SORTING.sorted(xs)` | Whether each item is at most the next, by `less_or_equal` |
| `SORTING.count(xs, x)` | How many items are `x` itself. It is ghost: contracts only |
| `SORTING.sort_array(a)` | The items of the `ARRAY` `a` in order, with `sort`'s contract on `a.to_list` |
| `SORTING.lower_bound(a, k)` | In a sorted array, the first position whose item is not below `k` |
| `SORTING.binary_search(a, k)` | In a sorted array, `Some` position of `k`, or `None` |

`sort`'s contract has three parts:
- `ordered`: `SORTING.sorted(Result)`.
- `same_length`: `Result.length = xs.length`.
- `permutation`: `forall x: K. SORTING.count(Result, x) = SORTING.count(xs, x)`. The result has the same items, each as many times.

`count` compares with `=`, which a contract may use on any type, so the permutation holds even where `equals` is not `=`, as for `STRING`.

The law `first_least(xs, j)` says that the first item of a sorted list is at most the item at `j`. Its proof chains `less_or_equal` along the list, which needs `equals` and `less_than` to agree. `ORD` does not promise that, so the law asks for it as the premise `agrees: forall a: K. forall b: K. a.equals(b) = (a = b)`. For a `NAT`, or any `CANONICAL_EQ` key, it holds at once:

```eiffel
  smallest(xs: LIST[NAT], j: NAT): NAT
    require
      inside: j < xs.length
    do
      check
        nat_agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)
        least: SORTING.first_least(SORTING.sort(xs), j)
      end
      SORTING.sort(xs).head
    ensure
      at_most: Result <= LIST_OPS.at(SORTING.sort(xs), j)
    end
```

The sort is stable: items that are `equals` but differ keep their order. A module that orders records by a key, as `less_than` on `Current.key < other.key`, gets them back in the order they came. This is tested, not proven (see the limits below).

In C, every step of the sort is a loop, so a list of millions of items cannot exhaust the stack. The two halves of each split are independent `let`s, so they run as parallel tasks. Each step builds new list cells, so a million `U64`s take about 2.7 s.

`sort_array` is the same merge sort on an `ARRAY`, and stable too. Each half is copied out with `slice` and sorted as a parallel task. The two sorted halves are then merged into a new array, which the merge loop fills with `set` in place. Its contract is `sort`'s, stated on `Result.to_list` and `a.to_list`. A program that fills and sorts a million `U64`s takes 0.68 s on one thread and 0.22 s on twenty, against 2.7 s with `sort` on a list. C's `qsort` on a plain array takes about 0.16 s in the same conditions.

`lower_bound` and `binary_search` search a sorted array in about log2(n) steps. Like `first_least`, they ask for the premise `agrees`, which a client states once in a `check`:

```eiffel
  position(a: ARRAY[NAT], k: NAT): NAT
    require
      ordered: SORTING.sorted(a.to_list)
    do
      check
        nat_agrees: forall p: NAT. forall q: NAT. p.equals(q) = (p = q)
      end
      match SORTING.binary_search(a, k)
        case Some(i) then i
        case None then a.length
      end
    end
```

`lower_bound`'s contract says that every item before the position it gives is below `k`, and that no item from it on is. `binary_search`'s says that what it finds is `k`, and that when it gives `None` no item of the array is `k`.

Limits:
- Stability is tested, not proven. For a key whose `equals` is not `=`, the proof needs `equals` and `less_than` to agree (`a.equals(b) and b.less_than(c) implies a.less_than(c)`). As a premise that quantifies over three values, the solver cannot use it.
- There is no `sort` for `SEQ`: a client cannot see how a `SEQ`'s model unfolds. Sort its items as a list, such as the one `s.fold_right` builds with `prepend`.
- `sort_array` is not in place. It copies each half, about n log2(n) items moved in all, and allocates about 2n arrays.
- A search needs `agrees`, and for `STRING` it does not hold: `equals` compares the bytes, and two strings with the same bytes are not known to be `=`. A client cannot prove the premise, so an array of strings can be sorted but not searched.

### Arrays: `ARRAY`

An `ARRAY[A]` holds a fixed number of items, read and changed by index.

| Routine | Gives |
| :--- | :--- |
| `ARRAY.make(n, x)` | `n` items, each `x` |
| `ARRAY.from_list(xs)` | The items of `xs` |
| `ARRAY.empty` | No items |
| `a.length` | The number of items |
| `a.at(i)` | The item at `i`, for `i < a.length` |
| `a.set(i, x)` | The array with `x` at `i`, for `i < a.length` |
| `a.swap(i, j)` | The array with the items at `i` and `j` exchanged |
| `a.slice(lo, hi)` | The items from `lo` up to, not including, `hi`, as a new array |
| `a.to_list` | The items, as a list |
| `a.fold(f, init)` | The items combined from the first, as `LIST_OPS.fold_left` |

A proof sees an array as the list of its items, `a.model`, and every contract is stated on it. `set`'s says `Result.model = LIST_OPS.update(a.model, i, x)`. Two laws, applied wherever a changed array is read, say what `at` gives after a `set`:
- `at_set_same`: `a.set(i, x).at(i) = x`.
- `at_set_other`: `a.set(i, x).at(j) = a.at(j)` when `i != j`.

So a client proves this from the contracts alone:

```eiffel
  put_read(a: ARRAY[NAT], i: NAT, x: NAT): NAT
    require
      inside: i < a.length
    do
      a.set(i, x).at(i)
    ensure
      same: Result = x
    end
```

An index past the end is refused by `at`'s and `set`'s `in_bounds`. There is no `=` on arrays in running code, as for `MAP`: compare `a.to_list`.

In C an array is one block, so `at` and `length` take constant time. `set` and `swap` change the array in place when nothing else holds it, and copy it otherwise. Nothing else holds it when the caller is done with it:
- the array is the result of another call, as in `ARRAY.make(n, 0).set(0, 1)`;
- it is a `let` that nothing after the `set` reads;
- it is a parameter that nothing after the `set` reads. A loop owns its parameters from its second iteration, and from its first when its caller gave it the array.

Every step of this loop changes the array in place, and `fill(ARRAY.make(1000000, 0), 0)` makes no copy:

```eiffel
  fill(a: ARRAY[NAT], i: NAT): ARRAY[NAT]
    require
      inside: i <= a.length
    variant
      a.length - i
    do
      if i = a.length then
        a
      else
        fill(a.set(i, i), i + 1)
      end
    ensure
      len: Result.length = a.length
    end
```

`let b = a.set(i, i)` then `fill(b, i + 1)` is the same. A million `set`s through it take about 15 ms, the allocation of the array included.

A copy is never wrong, only slow, and nothing warns you of it. Keep the old array out of anything after the `set`:
- `let b = fill(a, 0)` followed by a read of `a` copies `a` once, at the first `set`, since `a` is still needed. That is right when you want both.
- `a.set(i, x).set(j, a.at(k))` copies at the first `set`: the second call reads `a` after it. Read first: `let y = a.at(k)`, then `a.set(i, x).set(j, y)`.
- In a loop, an argument that reads the array beside the one that changes it, as `loop(a.set(i, x), a.length)`, makes every step copy. Read `a.length` inside the loop instead.
- In a `match` on something read from the array, as `match a.at(i)`, no arm changes the array in place: the names the `match` binds still point into it.

A record with a field of type `ARRAY` gives up its array in the same cases. `HEAP.push` is written `{ slots = HEAP_ORDER.push(Current.slots, Current.n, x), n = Current.n + 1 }`, and a loop `fill(h.push(x), n - 1)` changes the array in place from its second iteration. The routine must return its own type, and must build the record as its result, reading the field once and `Current` only through its fields.

### Sets, Queues and Heaps: `SET`, `QUEUE` and `HEAP`

| Routine | Gives |
| :--- | :--- |
| `SET.empty`, `s.add(k)`, `s.remove(k)` | A set of `ORD` keys |
| `s.has(k)`, `s.size`, `s.items` | Whether `k` is there, how many keys, and the keys in ascending order |
| `s.union(t)`, `s.intersection(t)` | The keys of either, and of both |
| `QUEUE.empty`, `q.push(x)`, `q.pop` | A queue, with `x` last, and without its oldest item |
| `q.peek`, `q.size`, `q.items` | The oldest item or `None`, how many, and the items, oldest first |
| `HEAP.empty`, `h.push(x)`, `h.pop_min` | A heap of `ORD` items, with `x`, and without one of its least items |
| `h.peek`, `h.size` | The least item or `None`, and how many |

A `SET` is proven through laws on `has`, as `MAP` is on `get`: `has_add_same`, `has_add_other`, `has_remove_same`, `has_remove_other`, `has_union` and `has_intersection`. `size_add_new`, `size_add_at_most`, `size_remove_absent` and `size_remove_at_most` count. `has`, `add` and `remove` take time linear in the size, and `union` and `intersection` the product of the two sizes.

A `QUEUE`'s contracts say what each operation does to `items`: `push` appends, `pop` takes the tail, `peek` is the head. Every operation but `items` takes amortized constant time.

A `HEAP` is proven through a ghost `count`: how many items are a given value. `count_push` adds one, `count_pop` takes away one of `h.peek.get`, and `peek_least` says that no item is below `peek`. `push` and `pop_min` need `agrees`:

```eiffel
  least(h: HEAP[NAT], x: NAT): BOOL
    require
      room: h.size < 1000
    do
      check
        agrees: forall p: NAT. forall q: NAT. p.equals(q) = (p = q)
      end
      let g = h.push(x)
      check
        counted: g.count(x) > 0
      end
      not x.less_than(g.peek.get)
    ensure
      holds: Result
    end
```

In C a heap is an array that `push` and `pop_min` change in place when the caller is done with the heap, in a logarithmic number of steps. A `push` on a full array makes one about twice as long.

Limits:
- `push` on a queue or a heap asks for room: `size < 18446744073709551615`.
- `HEAP` needs `agrees`, which does not hold for `STRING`, so there is no heap of strings.
- A proof about a fold or a count that uses `+%` can be slow: the solver reasons about wrapping addition bit by bit.

### Endless Sequences: `GENERATOR` and `UNFOLD`

A `GENERATOR[S, A]` is an endless sequence held as a finite value: a `state: S` and a step `next: S -> TUPLE[A, S]` that gives the item at a state and the state after it. `take(n)` is the first `n` items.

```eiffel
  naturals(from: NAT): GENERATOR[NAT, NAT]
    do
      GENERATOR.make(from, fn(s: NAT): TUPLE[NAT, NAT] do (s, s +% 1) end)
    end

  fibs(a: NAT): GENERATOR[TUPLE[NAT, NAT], NAT]
    do
      GENERATOR.make((a, 1), fn(p: TUPLE[NAT, NAT]): TUPLE[NAT, TUPLE[NAT, NAT]] do (p.first, (p.second, p.first +% p.second)) end)
    end

  tenth: NAT
    do
      fibs(0).nth(9)
    ensure
      is_34: Result = 34      -- proven from the contracts of `nth` and `make`
    end
```

A step never ends, so you choose what it does at the end of a bounded type: wrap with `+%`, as above, or count with `BIGNAT`. A step written `s + 1` over `NAT` is refused as an overflow.

What you can do with a generator:
- **Read it:** `head`, `tail`, `take(n)`, `nth(n)` and `drop(n)`.
- **Build one:** `make(s, f)`, `GENERATOR.iterate(x, f)` (`x`, `f(x)`, `f(f(x))` and so on), and `GENERATOR.cycle(xs)` for a non-empty list.
- **Combine:** `map(f)`, `zip(other)`, `zip_with(other, h)` and `scan(f, init)`. `zip`'s state is the pair of both states.
- **Search, with a bound:** `find_within(k, p)` and `take_while_within(k, p)` look at the first `k` items only.

There is no `filter`. A generator's next item that passes might never come, so a filter could run forever. Take a prefix first, and filter that. B&W §7.3's sieve on a bounded prefix:

```eiffel
  sieve(xs: LIST[NAT]): LIST[NAT]
    variant
      xs.length
    do
      if xs.length = 0 then
        LIST.empty
      else
        let p = xs.head
        sieve(LIST_OPS.filter(xs.tail, fn(x: NAT): BOOL do if p = 0 then True else x % p != 0 end end)).prepend(p)
      end
    end
```

`sieve(LIST_OPS.range(2, 30))` gives the ten primes below 30.

Laws let a client reason about a prefix with `LIST_OPS`' laws:
- `nth_take(g, n, k)`: for `k < n`, `g.nth(k) = LIST_OPS.at(g.take(n), k)`.
- `take_split(g, n, m)`: for `m <= n`, `g.take(n) = g.take(m).append(g.drop(m).take(n - m))`.
- `map_take(g, f, n)`: `g.map(f).take(n) = LIST_OPS.map(g.take(n), f)`.

Each combinator states its step (`stepping`), so `GENERATOR.iterate(x, f).next(s)` is known to be `(s, f(s))`. `cycle` is the exception: it states only where it starts.

**What holds of every item.** A property holds of every item when it holds of the first state and the step keeps it. That is an induction on the number of items. Here it bounds a retry schedule:

```eiffel
  delays: GENERATOR[NAT, NAT]
    do
      GENERATOR.iterate(100, fn(d: NAT): NAT do if d >= 5000 then 10000 else d * 2 end end)
    end

  capped(g: GENERATOR[NAT, NAT], n: NAT): BOOL
    require
      starts: g.state >= 0 and g.state <= 10000
      keeps: forall d: NAT. d <= 10000 implies
        (g.next(d).first >= 0 and g.next(d).first <= 10000 and g.next(d).second >= 0 and g.next(d).second <= 10000)
    variant
      n
    do
      if n = 0 then True else capped(g.tail, n - 1) end
    ensure
      holds: Result
      bounded: forall k: NAT. k < n implies (g.nth(k) >= 0 and g.nth(k) <= 10000)
    end
```

A call `capped(delays, k + 1)` in a `check` then proves that `delays.nth(k)` is at most 10 s. A generic version over any property needs a concept bound on the state, which this library does not have yet.

**A generator's items are in their type's range.** The verifier knows this of any generator, not only of a prefix it can unfold. So a routine can return `g.take(n)` as a `LIST[NAT]`, or `g.nth(k)` as a `NAT`, for any `g`, `n` and `k`. The combinators carry ranges too. For example, `naturals(1).map(f)`'s items are in `f`'s result range.

The verifier states this range over the state and the item together. The state is in its type's range, and the step takes a state in range to an item in range and a next state in range. Every lambda and every function parameter is proven to keep that promise, so every generator built from them does too.

A bound other than the type's, such as `<= 10000` above, still needs a property like `capped`.

An `UNFOLD[S, A]` is a sequence that may end, like Elixir's `Stream.unfold`: its step gives an `OPTION`, and `None` ends it. `take(n)` gives at most `n` items. `to_list(measure)` gives all of them, if you prove the sequence ends: `measure` of the state is never negative, and it shrinks at each step. Over a `NAT` state, "never negative" quantifies over every integer, so a measure such as a list's length is the easy case.

Compared with Elixir's `Stream` (`unfold`, `iterate`, `cycle`, `Enum.take`):
- A search that may never end is refused, where Elixir would loop.
- Overflow is the client's choice, not a bignum.
- The state's type is visible in the generator's type, as there are no existential types.
- There are no effects; those belong to reactive programs.

### Multi-File Projects

Give `gardelc` several files, a directory (searched recursively, skipping `build/` and `target/`), or a wildcard pattern, and they form **one project** with a flat module namespace:

```bash
gardelc src/                  # every .gdl file under src/
gardelc src/*.gdl --strict    # a wildcard pattern
gardelc a.gdl b.gdl           # explicit files
```

Existing examples and projects work directly without any configuration file.

Rules:
- **One module per file**, named after it: `module BANK_ACCOUNT` lives in `bank_account.gdl`. Otherwise you get `W0201: file_module_name_mismatch`, which `--strict` turns into a failure.
- **No duplicates:** two files declaring the same module is `E0102`.
- **No cycles between files:** each file's proofs trust the contracts of the files it uses, so a cycle would let two modules prove each other. Cross-file cycles are rejected with `E0103: cyclic_module_dependency`; genuinely mutually dependent modules belong in one file.

### Project Manifests (`Gardel.toml`)

Projects may also declare their identity and boundaries with a `Gardel.toml` file in their root directory (Task E.1):

```toml
[project]
name = "echo-server"
version = "0.1.0"
```

When a manifest is used:
- Sources are placed under `src/` (e.g. `examples/echo_server/src/*.gdl`).
- Compile the project by passing `--project <DIR>`:
  ```bash
  gardelc --project examples/echo_server --strict
  ```
- Or run `gardelc` with no input operands from within the project directory:
  ```bash
  cd examples/echo_server
  gardelc --strict
  ```
- **Package naming rules:** Lowercase ASCII letters, digits, `-`, and `.`. Each segment must start with `a`–`z` and cannot end with `-`. The name cannot start with `gardel.` and cannot produce the default alias `GARDEL` (which is reserved for the Standard Prelude).
- **Strict TOML 1.0:** The parser enforces a clean, dependency-free subset with 1-based line numbers on syntax or schema errors.
- **Zero semantic difference:** A manifest with no dependencies changes nothing about how source code behaves or verifies; it establishes project identity and boundaries. Projects without a `Gardel.toml` continue to compile using files and directory paths as usual.

### Dependencies

A package lists the packages it uses in `[dependencies]`, and then uses their modules by their bare names, with no import statement. `examples/hello_api` is a REST service built on the `grest` package. It lists `ghttp` and `gjson` too, because it names their types (`HTTP_RESPONSE`, `JSON`) and a package sees only the packages it lists:

```toml
[project]
name = "hello-api"
version = "0.3.0"

[dependencies]
grest = { path = "../../grest" }
ghttp = { path = "../../ghttp" }
gjson = { path = "../../gjson" }
"acme.json" = { path = "../json", as = "J" }   # a dotted name is quoted, and `as` sets the alias
```

```bash
gardelc examples/hello_api --strict
```

What you can rely on:

- **Your own names win.** A module in your project is never replaced by one in a dependency, so a dependency adding a module cannot change what your code means. Inside a package, its own names win in the same way.
- **Two packages can use one name.** Each package keeps its own `PARSER` and its own `Leaf`, with its own proofs. A build fails only if you use such a name bare and two of your dependencies provide it (`E0105` for a module, `E0112` for a constructor). The prelude counts as a provider too.
- **Only what you list is in scope.** Your dependency's own dependencies give you no names, though their types can reach you through its routines.
- **You see contracts, not bodies.** A proof in your code can rest only on a dependency's `ensure` clauses. A dependency's body can change inside its contract without re-proving your code.
- **Nothing is taken on trust.** A dependency is verified from source on first use and cached. If it fails, your build fails. `--strict` refuses an `assume` in any package, and a plain run reports them.
- **A dependency's `program` is not yours.** It is verified but not built, and you cannot refer to it.

Diagnostics name a dependency's module by its full name, such as `ghttp::HTTP_PARSER`. Aliases have no effect on proofs or generated code.

### A Package for JSON: `gjson`

`gjson/` is a verified JSON package (RFC 8259). Add it to `[dependencies]` as `gjson = { path = "../gjson" }`, with the path to your copy, and use `JSON`:

```eiffel
match JSON.parse_text("{\"b\": [1, 2.5, \"x\"], \"a\": null}")
  case Ok(v) then v.to_text            -- {"a":null,"b":[1,2.5,"x"]}
  case Error(e) then NUMBER_TEXT.nat_to_text(e.offset)   -- the byte where it went wrong
end
```

- **The value.** `JSON` is opaque. Build it with `null`, `bool`, `int`, `number`, `string`, `empty_array`, `empty_object`, `with_item` and `with_field`, or an array from a list with `JSON_ARRAY.of`, and read it with `is_null`, `as_bool`, `as_int`, `number_text`, `as_string`, `count`, `item`, `field` and `depth`. Its invariant keeps text valid UTF-8, the keys of an object unique and in ascending byte order, and nesting at most 64 levels, so `with_item` and `with_field` return an `OPTION`. An integer in the range of `I64` is an `I64`, and any other number keeps its text as read.
- **Parsing.** `JSON.parse(b: BYTES)` and `JSON.parse_text(s: STRING)` are total and return `RESULT[JSON_ERROR, JSON]`. Every error names a byte offset (`e.offset`). Input over 4 MiB is `TooLarge`, nesting over 64 levels is `TooDeep`, and a repeated key is `DuplicateKey`.
- **Writing.** `to_bytes` and `to_text` add no white space and write keys in order, so equal values give equal bytes.
- **What you can prove.** The law `JSON.scalar_round_trip` says a scalar read back from its own bytes is itself. With the builders' contracts it reaches your code with no extra work:

```eiffel
back(i: I64, s: STRING): BOOL
  require
    short: s.byte_length <= 100000
  do
    True
  ensure
    int: JSON.parse(JSON.int(i).to_bytes) = Ok(JSON.int(i))
    text: JSON.parse(JSON.string(s).to_bytes) = Ok(JSON.string(s))
  end
```

The round trip of arrays and objects is tested (`gjson/tests/cases.txt`) but not proven. `gjson/README.md` lists the errors, the costs and what is proven.

### A Package for Terminal Interfaces: `gtui`

`gtui/` is a verified library for full-screen terminal programs, in the style of Elm. Add `gtui = { path = "../gtui" }`, give `TUI` an `update` and a `view`, and the reactive program is two lines:

```eiffel
update(s: TODO_STATE, e: EVENT): TUI_NEXT[TODO_STATE]    -- Next(state, cmds) or Quit(code, after)
view(s: TODO_STATE, blank: SCREEN): SCREEN                -- draws on a blank screen of the terminal's size

init(args: SEQ[STRING]): STEP[TUI[TODO_STATE]] do TUI.start(TODO_STATE.empty, update, view) end
step(t: TUI[TODO_STATE], e: EVENT): STEP[TUI[TODO_STATE]] do t.step(e) end
```

- **The loop.** `start` takes the alternate screen, hides the cursor, turns on bracketed paste, asks the size and reads a key. Every event but `Shutdown` goes to `update`, a paste as one `Pasted(text)`, and the screen is drawn again from `view`: whole after a resize, and otherwise only the cells that changed, in one `Write`. `update`'s commands that would write on the screen or read the terminal are dropped, except `TUI_COMMANDS.copy(text)`, which puts the text on the terminal's clipboard (OSC 52). `Quit` and `Shutdown` put the terminal's screen back.
- **Drawing.** `SCREEN.text(clip, x, y, text, style)` and `fill(clip, c, style)` change only cells inside `clip` and the screen. A cell holds no control character. Text takes the columns a terminal gives it: a CJK character two cells, a combining mark none (`CHAR_WIDTH`, generated from the Unicode data), and a wide character cut in half by a clip or another write leaves a space. A style has bold, dim, underlined and reversed, and a foreground and a background from the terminal's 16 colours: `STYLE.plain.fg(COLOR.cyan).bold`. `screen.with_cursor(x, y)` shows the terminal's cursor, as a bar, at that cell, where typing goes; a screen hides it otherwise.
- **Layout.** `RECT`'s cuts (`take_top(n)`, `drop_top(n)` and the like) and `LAYOUT.rows(r, sizes)` and `columns(r, sizes)`, with `SIZE.fixed(n)` and `SIZE.flex(weight)`. Take a child with `LIST_OPS.at(parts, i)`: the contract gives one per size.
- **Widgets.** `TEXT_VIEW`, `LIST_VIEW` (with a selection), `STATUS_BAR` and `INPUT_LINE`; `FRAME`, a box with a title, and `FOCUS`, which pane has the keys; `TABS` (`Text(t) | Short(s)`), `TREE_VIEW`, nodes that open and close, and `PALETTE`, commands narrowed as one types after `/`. `TEXT_VIEW.draw_code` shows source: line numbers, a marked line, and the `SPAN`s a function of each line gives, such as a lexer's. Each draws in a rectangle and takes keys with `on_key`.
- **Editing.** `TEXT_AREA.of_text(t)` is a text editor in a rectangle: `a.on_key(k, r)` and `a.draw(s, r, st, gutter, chosen)`, with line numbers, a selection by Shift, cut, copy and paste by Ctrl-X, Ctrl-C and Ctrl-V, or Shift-Delete, Ctrl-Insert and Shift-Insert where Ctrl-C is a signal, as in the Gardel runtime (`TUI_COMMANDS.copy(a.clipboard)` when `a.just_copied`), undo and redo by Ctrl-Z and Ctrl-Y, `go_to_line(n)`, `find(pattern)`, and `a.paste(t)` for a `Pasted`. `with_lexer(lex)` and `draw_code(..., spans)` highlight it by line, each line lexed from the state the line before ends in, so a string can run over lines. The text is a zipper of lines, so a key costs one line: about 0.2 ms with its frame on 5000 lines.
- **Running a child.** A `RunAttached` from `update` gives the terminal to the child: `TUI` puts the screen back first, draws nothing while it runs, and takes the screen again when it ends.
- **What is proven.** Every write of a frame lies inside the screen (`SCREEN_DIFF.changes`'s `within`). A layout's children lie inside their parent and no two overlap: `LAYOUT.across_inside` and `LAYOUT_APART.columns_apart` reach a client, as `tests/gtui.rs` shows. Text cut into lines comes back whole (`LINE_SPLIT`); an edit undone gives back the text and the cursor; the cursor is in the text and on the page drawn; and the selection is inside the text. That a diff's writes give the new screen is tested, not proven.

`examples/todo_tui` is a small program, and `examples/gtui_tour` a workbench of three panes that uses every widget. A full redraw of 200 by 60 takes about 7 ms. `gtui/README.md` has the details.

`tools/tablon` is a workbench for a Gardel project, written in Gardel on `gtui` and `gjson`. Build it from `tools/tablon` with `gardelc --project . --strict`, as `gardelc` writes `build/` under the directory it runs in, and run `tools/tablon/build/tablon_bin DIR [GARDELC] [EDITOR]` in a terminal, where `DIR` is a project, `GARDELC` the compiler, `gardelc` on the `PATH` when it is not given, and `EDITOR` the command line that edits a file, `vi` when it is not given. It reads the project's modules from `gardelc --inspect --json-diagnostics --no-verify`, so it browses a project whose proofs fail. Tab moves between the panes; `m` and `f` choose Modules or Files on the left; `t`, `s`, `f`, `n`, `l`, `i`, `a`, `d`, `c` and `u` choose a view on the right (text, short form, features, creators, laws, invariants, concepts, instances, clients, suppliers); Enter opens a node, a part's line or a module; `/` opens the commands; `q` quits. Ctrl-K checks the project, and Problems lists its failures, Enter going to each one's line; Ctrl-B builds it; Ctrl-R builds and runs it, an ordinary program with its output in Run and the keys typing to its input, a reactive one attached; in Text, Enter edits the file shown and Escape stops, Ctrl-S saves it and checks again, and quitting with unsaved files asks first; Ctrl-E edits the file shown in your own editor, then checks again; Ctrl-X stops what runs; `/new NAME` writes a new module's file under `src`, where there is none, opens it and reads the modules again. `tools/tablon/README.md` has the details.

### Testing a Package: `gardelc test` and `gtest`

A package's tests are a package of their own in its `tests/` directory, with a `Gardel.toml` that depends on the package, so they see only its contracts. Each `program` in it is a test. `gardelc test DIR` verifies that package under `--strict` and builds it, once, runs each program in `DIR/tests` with `tests/cases.txt` on its standard input when there is one, and reports each by its exit status. A `reactive program` is a test too, and one named `P` with a file `tests/p.events` runs from the script in it, as from `GARDEL_EVENTS`; lines that start with `#` are skipped, and a line that starts with `> ` is a `Line` event with the rest of it. Its own exit status is 0 when every program passed, and 1 when one failed or the tests did not verify or build. A failing program's output is shown under its name.

```text
$ gardelc test gjson
[test] `gjson\tests` verified: 0 modules verified now, 23 proven earlier on this machine (1.9 s)
[test] GJSON_TEST ... ok (1.2 s)
[test] 1 program: 1 passed, 0 failed (3.1 s)
```

`--filter NAME` runs only the programs whose name contains `NAME`. The package and its tests share one build and the proof cache, and an executable whose C file has not changed is not compiled again, so a run with nothing changed takes a few seconds. A program that runs for ten minutes is stopped and fails.

The package `gtest` (in `gtest/`) is what a test program is written with. Add `gtest = { path = ".../gtest" }` to the tests' manifest.

- **Checks.** `CHECK.that(name, b)`, `equal_text`, `equal_nat`, `equal_int`, `equal_shown(name, got, expected, same, show)`, `is_ok`, `is_error`, `is_some` and `is_none` each give a `TEST_RESULT`: `Pass(name)` or `Fail(name, message)`, where the message says what was expected and what came, as `expected 4, got 3`.
- **The runner.** IO belongs to a `program`, so its `main` prints `TESTS.report(rs)`, each failure with its name and then the count, and exits with `TESTS.exit_code(rs)`:

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

- **Cases files.** `CASES` reads a line `input => expected`. Empty lines and lines that start with `#` are skipped, and `%XX` stands for a byte. `CASES.check(rs, n, line, run)` adds the result of line `n`: `run(input)` compared with the expected text. A program reads its lines with `IO.read_line` and checks each, as `gjson/tests/src/gjson_test.gdl` does; `CASES.bytes(s)` gives an input that need not be text.
- **Property tests.** `PROPERTY.for_all(name, gen, n, seed, prop)` checks `prop` on `n` values of a generator made from `seed`. A failure says which case and the seed it was made from, so a run from that seed makes it first; `for_all_shown` also shows the value. The values come from SplitMix64 (`SPLIT_MIX`), pure and the same on every system. `GENS` has `u64`, `nat_below(n)`, `nat_in(lo, hi)`, `bool`, `list(g, max)`, `text(max)`, `text_from(alphabet, max)`, `map` and `pair`, and `GEN.make` takes any step from a state to a value and the next state.
- **Reactive steps.** `SCRIPT.play(step, state, events)` runs a `step` over a list of events and gives `Running(state, commands)` or `Stopped(code, commands)`, with every command in order; `SCRIPT.printed(commands)` is the text of each `Print` and `Write`. The `step` must be a routine of a module, as a `reactive program` cannot be named. Keep the run in a `let`: a routine that returns a `SCRIPTED[NAT]` must show its state in range, and a generic routine's result has no range. Only the host can make a `CONN_ID`, so a test whose events need them is a `reactive program`: its events file begins with `Accepted:1 Accepted:2`, it keeps the connections it is given and makes its runs with them, and it stops with their exit code at `Shutdown`. `grest/tests/src/grest_server_test.gdl` does so, with each script on a `> ` line. A long text is better kept there than in a literal, which the solver has to read.

`gjson`, `grest`, and `gtest`'s own tests (`gtest/tests`) use it. `ghttp` still reads its cases from a Rust test.

### Mutating a Package: `gardelc mutate`

A proof shows that the code keeps its contract, and a test that it does what the test asks. Neither shows that the contract and the tests say enough. `gardelc mutate DIR` changes the package's code in small ways, one change at a time, and says what refuses each changed package (a *mutant*):

- **the verifier**, with the clause that fails: what the change breaks is proven;
- **a test**, with its name: it is tested, and not proven;
- **nothing**: the mutant *survives*, and no proof or test tells it from the original.

```text
$ gardelc mutate price
[mutate] 10 mutants
[mutate] the package as it is: verified and tested (4.1 s)
[mutate] 1/10 src/price.gdl:7 PRICE.doubled: `require` `small` dropped ... killed by the verifier: `overflow_add` of `doubled` (Arithmetic Overflow) (2.4 s)
[mutate] 2/10 src/price.gdl:16 PRICE.fee: `>` -> `>=` ... killed by a test: PRICE_TEST: FAIL paid: expected 50, got 0 (6.6 s)
[mutate] 3/10 src/price.gdl:21 PRICE.label: `5` -> `6` ... SURVIVED (4.8 s)
    - 5
    + 6
```

The changes are: an integer one more or one less; `True` for `False`; `<` for `<=`, `>` for `>=`, `=` for `!=` and `and` for `or`, each both ways; a condition negated; one branch of an `if` always taken; an arm of a `match` with the next arm's body; a `require` or `invariant` clause dropped; two arguments next to each other swapped; and a call replaced by its receiver or an argument. Only code is changed. A `check`, an `ensure`, a law's proof, a `specify` routine and a routine that states a fact (a `BOOL` with a clause that ensures `Result`) are left alone. Many changes are of the wrong type; they are refused before any proof and counted as "not a program".

Each mutant is made in a private copy, verified with `--strict`, its own module first, and then tested with `gardelc test` if the package has a `tests/` package. The exit status is 0 when none survived, 1 when one did or the package itself does not pass, and 2 for a wrong command line.

- `--only MODULE` changes only that module, and may be repeated. `--limit N` tries `N` of the mutants, chosen by `--seed` (0 when it is not given). `--list` lists them and tries none.
- `--jobs N` tries `N` at a time. A mutant can keep the solver busy for long: `--time-limit SECS` stops the verifier on one after that time (1800 when it is not given). A mutant the solver gives up on is refused, as any module it gives up on is, but it was not shown to be wrong, so it is counted apart, as "not proven".
- A survivor is an invitation to state or to test more. One that means the same as the original goes in `Gardel.mutants`, beside the manifest: one a line, as the report writes it, with the reason above it as a `--` comment. It is not tried again. A line that fits no mutant any more is reported.

### Checking a New Version

Before releasing a package, compare it with the version your clients use:

```bash
git worktree add ../json-1.0 v1.0     # any directory holding the old version
gardelc --compare-with ../json-1.0
```

The run builds and verifies the new version as usual, then exits 1 if a public item changed in a way that can break a client:

- a stronger `require`, a weaker `ensure`, or a weaker `invariant`
- a removed or less visible routine, law or module, or a changed signature
- a changed public `representation`, even one new constructor, because it breaks a complete `match`
- a concept the module no longer satisfies, or a renamed package
- a removed or changed public `concept`, in either direction, as clients both supply and rely on it
- a new `assume` anywhere in the build, even in a private routine, because `--strict` refuses it in every client
- a new package among your dependencies, or a dependency at another version, because a build holds one package of a name

A body change that adds no `assume` is not breaking, and neither is a new routine, a new law or a change to a hidden representation. Parameters are compared by position, so renaming one changes nothing, in a concept too.

A new module, concept or public representation is a warning, even under `--strict`. If a client already uses that name bare from another package, it now gets `E0105` or `E0112`, and one qualifier fixes it.

Clauses that differ go to the solver, and a change counts as compatible only if the solver proves it. It sees each routine as an unknown function and uses none of your contracts or laws, so a true implication that needs one of them is reported as breaking.

An `assume` the old version also had is the warning `rests_on_assume`, which lists them, as a proof may lean on one without any contract changing. Add `--strict` to refuse them.

A dependency whose text changed at the same version is the warning `dependency_moved`: give it a new version, and compare it with its old one.

### Qualified Names

Use the alias from your manifest, with `::`, when a bare name is ambiguous. `GARDEL` is the prelude's alias:

```
first_size(p: JSON::PARSER, q: LIST[XML::PARSER]): U64
  do
    p.size
  end

theirs: GARDEL::COMMAND
  do
    GARDEL::COMMAND.ReadLine
  end
```

A constructor can be named through its module, which also separates two modules of one project:

```
let a = TOK_A.Leaf(1)
let b = JSON::TOK.Leaf(2)

match r
  case RESULT.Error(e) then e
  case Ok(v) then v
end
```

A bare constructor means the module's own first, then one from its project, and only then one from the prelude or a dependency. If that still leaves several, the type of a `match` scrutinee, or the declared result of a routine that returns the constructor directly, chooses. So a dependency that declares `Error` does not disturb your `RESULT` matches. Anywhere else the compiler stops with `E0112` and shows the spellings. The type only breaks a tie: a module with its own `Stop` writes `STEP.Stop(0, cmds)` for the prelude's.

A qualifier never opens anything. A hidden member stays hidden however it is spelled.

A `let` can state its type, which is checked. This also settles a generic value that nothing else pins down:

```
let s: STACK[U64] = STACK.empty
```

### Name Resolution

There are no import statements. Before anything is verified, `gardelc` resolves every name in the run against the project, its direct dependencies and the prelude, and reports each failure at its source location:

| Rule | Meaning |
| :--- | :--- |
| `E0106: unknown_module` | A module that is not in the run or the prelude |
| `E0107: unknown_type` | A type that is not declared |
| `E0108: unknown_routine` | A routine or field the module does not have |
| `E0109: unknown_constructor` | A constructor no module declares |
| `E0110: unknown_variable` | A name that is not a parameter, local, field or routine |
| `E0111: wrong_argument_count` | A call with the wrong number of arguments |
| `E0105: ambiguous_module_reference` | A bare module, type or concept that two dependencies, or a dependency and the prelude, provide. The message gives the qualified spellings |
| `E0112: ambiguous_constructor` | A constructor declared by two other modules, or provided by two dependencies, with no known type to choose. The message gives the qualified spellings. A module's own constructor always wins, and a dependency's wins over the prelude's, so the prelude can grow without breaking your code: with `ghttp`, a bare `Delete` is `HTTP_METHOD.Delete`, and `GARDEL::KEY.Delete` is the key. |
| `E0113: hidden_member` | A member another module does not export: a private routine, a hidden field, or a constructor of a hidden representation, which can be neither built nor matched |
| `E0114: export_outside_package` | An export list naming a module outside the package. `{EQ}`, naming a concept, stays allowed |
| `E0115: reserved_name` | A declared name of the form `PK<n>_...`, which is how a dependency's names are held internally |
| `E0116: let_type_mismatch` | A `let x: T = e` whose value is known to have another type, or an integer literal that does not fit `T` |
| `E0117: argument_type_mismatch` | An argument of another type than its parameter, such as `xs.append(4)`: `append` joins two lists, so the element goes in `prepend`. Integer types pass for one another where the range allows |
| `E0118: result_type_mismatch` | A result of another type than the routine declares, such as `xs.head` for a `LIST[NAT]`, or one branch of an `if` that gives a `BOOL` where the other gives a number. Integer types pass for one another where the range allows |
| `E0119: needs_an_instance` | `BOX.get` where `get` is a `feature` of `module BOX`: it works on a `BOX` value, so call it on one, `b.get`. Move a routine that needs no value to `create`, which is called as `BOX.make(3)` |
| `E0199: verifier_translation_failed` | The verifier could not accept what `gardelc` generated. The module is not verified. This is a limit of the compiler, not a fault shown in your program, and `GARDEL_KEEP_BPL=1` keeps the generated file. |

Exporting to a named module across files, as in `feature {OTHER}`, is not usable yet: the export list counts as a dependency, so it forms a cycle (`E0103`).

### The Standard Library Sources (`stdlib/`)

The prelude's source code lives in the repository's `stdlib/` folder, one module or concept per file, and is embedded into `gardelc` when the compiler is built:

```text
stdlib/
├── concepts/      eq.gdl, canonical_eq.gdl, ord.gdl
├── core/          option.gdl, result.gdl, io_error.gdl, step.gdl,
│                  command.gdl, event.gdl, key.gdl, conn_id.gdl, proc_id.gdl, dir_entry.gdl
├── collections/   list_ops.gdl, seq.gdl
├── numeric/       bignat.gdl
├── text/          string.gdl, bytes.gdl, string_error.gdl
└── functional/    function_ops.gdl
```

When working on the standard library itself:
- **Verify it with `gardelc --stdlib --strict`.** This always verifies the tree `gardelc` was built from (it prints which one), never a `stdlib/` folder that happens to be in the current directory.
- **Any stdlib file means the whole tree.** `gardelc stdlib` or `gardelc stdlib/core/option.gdl` verify the whole tree too: the modules depend on each other (`RESULT` uses `OPTION`, everything uses the concepts), so a file on its own could not be checked.
- **Stdlib files are recognized by their location**, so an edited file is still treated as part of the prelude. Stdlib files cannot be mixed with user files in one run.
- **Rebuild after editing.** Programs get the copy embedded at build time. After editing a stdlib file, `--stdlib` notes that it differs from the embedded copy; run `cargo build` so programs see the change.
- **The file list must match.** If the files on disk and the embedded list disagree (a new file not yet added to `STDLIB_SOURCES` and `phase4_stdlib_source()` in `gardelc/src/main.rs`, or a missing one), `--stdlib` fails with an error instead of verifying a different set than it ships.
- **`gardelc --emit-stdlib [DIR]`** writes the embedded copy to `build/stdlib/` (or `DIR`). It never overwrites a file whose content differs: it refuses and writes nothing.

---

## 18. Compiler CLI Cheatsheet & Grammar Reference

### Building & Installing from Source

```bash
# Clone the repository
git clone https://github.com/gardel-lang/gardel.git
cd gardel/gardelc

# Build release binary and run test suite
cargo build --release
cargo test

# Install gardelc to PATH (via Cargo binary directory)
cargo install --path .

# Verify installation by checking the Standard Prelude
gardelc --stdlib --strict
```

### CLI Commands

```bash
# Verify all contracts, concept laws, and proofs statically (emits Boogie, Bend, C11 to build/)
gardelc <file.gdl>

# Verify a multi-file project: several files, a directory, or a wildcard pattern
gardelc <file.gdl> <other.gdl> ...
gardelc <DIR>
gardelc 'src/*.gdl'

# Compile and verify a manifest project (defined by Gardel.toml)
gardelc --project <DIR>
gardelc examples/echo_server   # a directory with a Gardel.toml is the same project
gardelc                        # when ./Gardel.toml is present

# Verify the Standard Prelude sources (stdlib/) as one linked project
gardelc --stdlib --strict

# Run verification with structured JSON diagnostics for AI agents. A program that verifies is
# built as without the flag: a C compiler that fails is `E0401`, and a `note` names the executable
gardelc <file.gdl> --json-diagnostics

# Fix the solver seed, to check that a proof is stable, or its work limit per procedure.
# Without a seed, a module on which the solver gives up is verified again under seeds 1 to 3
gardelc <file.gdl> --z3-seed <N>
gardelc <file.gdl> --z3-rlimit <N>

# Audit all 'assume' blocks in the file or project
gardelc <file.gdl> --audit

# Reject any 'assume' blocks (strict verification mode)
gardelc <file.gdl> --strict

# Inspect public contract interface of a module (like Eiffel 'short')
gardelc --inspect <file.gdl>
gardelc --stdlib --inspect [MODULE]

# The same as one JSON object, for a tool: each module's features, laws, invariants, source
# lines, suppliers and concepts (spec §12)
gardelc --inspect --json-diagnostics

# Short forms while proofs fail: types checked, nothing proven, marked as not verified
gardelc --inspect --no-verify

# Unfold recursive definitions to depth N for the solver (default 2)
gardelc <file.gdl> --fuel <N>

# Compile without the Standard Prelude (bootstrap mode)
gardelc <file.gdl> --no-prelude

# Bounded symbolic checking instead of the Boogie/Z3 proof. It samples, and proves nothing:
# each module gets a warning, `--strict` refuses it, and `--inspect` shows no interface.
# Without this flag, a run with no working Boogie or Z3 is an error.
gardelc <file.gdl> --bounded-check

# Run the solver on every module and prove the prelude, ignoring the local verification cache (for CI)
gardelc <file.gdl> --verify-deps

# Compile, verify, and run the executable
gardelc <file.gdl> --run

# Report what would break a client of an older version of this package
gardelc --compare-with <old-dir>

# Verify and build DIR/tests (default ./tests), run each program in it with tests/cases.txt
# on its standard input, or from tests/<program>.events, and report each by its exit status
gardelc test [DIR] [--filter <NAME>]

# Change the package's code in small ways, and report the changes that neither its proofs
# nor its tests refuse
gardelc mutate [DIR] [--only <MODULE>] [--limit <N>] [--seed <N>] [--jobs <N>] [--time-limit <SECS>] [--list]

# Check MODULE first, and stop at the first module with an error
gardelc <file.gdl> --fail-fast <MODULE>
```

`gardelc` remembers what this machine has proven, in `GARDEL_CACHE_DIR` or your user cache directory. A module whose verification condition has not changed is not given to the solver again, and the run says how many came from the cache. Changing `gardelc`, Boogie, Z3 or a limit verifies everything again. If Boogie or Z3 cannot give its version, the run uses no cache, and a `[cache] off` line names the tool and the reason. All other checks run every time, so `--strict` cannot be answered by an earlier run. The cache only saves time: deleting it is always safe.

A normal run reports five steps:

```text
[0/5] Standard Prelude injected (or bootstrap mode)
[1/5] Parsed / linked the input files
[2/5] Checked visibility, concept laws, IO boundary, and Boogie/Z3 verification -> errors and warnings
[3/5] Emitted Boogie VCs        -> build/<module>.bpl
[4/5] Emitted Bend v2 target    -> build/<module>.bend
[5/5] Emitted C11 runtime target -> build/<module>.c
```

A `program` or a reactive program is then compiled to `build/<module>_bin`, or `build/<module>_bin.exe` on Windows. Both shells there also run it as `build/<module>_bin`. A C file is written only when its text changes, and an executable no older than its C file is not compiled again: the run then says `Native executable up to date`.

```bash
# Write the Standard Prelude embedded in gardelc to build/stdlib/ (or --emit-stdlib <DIR>); never overwrites edited files
gardelc --emit-stdlib
```

### Formal Grammar (EBNF Summary)

```ebnf
File          ::= GroupDecl? ( ModuleDecl | FunctionsDecl | ProgramDecl | ConceptDecl )+

ModuleDecl    ::= "module" UpperIdent TypeParams? Satisfies? WhereClause?
                    ( RepresentationSection | CreateSection
                    | FeatureSection | SpecifySection )*
                    InvariantBlock?
                  "end"

FunctionsDecl ::= "functions" UpperIdent ( FeatureSection | SpecifySection )* "end"
ProgramDecl   ::= "reactive"? "program" UpperIdent FeatureSection* "end"
ConceptDecl   ::= "concept" UpperIdent TypeParams? Refines?
                    ( Signature | RoutineDecl | LawDecl )*
                  "end"

Satisfies     ::= "satisfies" UpperIdent ( "," UpperIdent )*
WhereClause   ::= "where" UpperIdent "satisfies" UpperIdent
                    ( "," UpperIdent "satisfies" UpperIdent )*
TypeParams    ::= "[" UpperIdent ( ":" UpperIdent )? ( "," UpperIdent ( ":" UpperIdent )? )* "]"

VisList       ::= "{" UpperIdent ( "," UpperIdent )* "}"
RepresentationSection ::= "representation" VisList ( FieldDecl+ | VariantDecl+ )
CreateSection ::= "create" VisList RoutineDecl+
FeatureSection::= "feature" VisList RoutineDecl+
SpecifySection::= "specify" VisList ( RoutineDecl | LawDecl )+

RoutineDecl   ::= LowerIdent TypeParams? Params? ":" Type
                    RequireBlock? VariantBlock?
                    "do" Block
                    EnsureBlock?
                  "end"

LawDecl       ::= "law" LowerIdent Params
                    RequireBlock? TriggerBlock? VariantBlock?
                    ( "do" Block )?
                    EnsureBlock?
                  "end"

RequireBlock  ::= "require" TaggedPred+
EnsureBlock   ::= "ensure" TaggedPred+
InvariantBlock::= "invariant" TaggedPred+
VariantBlock  ::= "variant" Expr ( "," Expr )*
TaggedPred    ::= LowerIdent ":" Expr

Block         ::= ( Stmt ( "\n" | ";" )+ )* Expr
Stmt          ::= "let" LowerIdent ( ":" Type )? "=" Expr
                | CheckBlock
                | AssumeBlock
                | "reveal" LowerIdent

Expr          ::= Literal | LowerIdent | UpperIdent Args? | Expr "." LowerIdent Args?
                | Expr BinOp Expr | UnOp Expr | "(" Expr ( "," Expr )* ")"
                | "if" Expr "then" Block "else" Block "end"
                | MatchExpr | LambdaExpr | WithIoExpr
                | RecordLit | RecordUpdate | QuantExpr

MatchExpr     ::= "match" Expr ( "case" Pattern "then" Block )+ "end"
LambdaExpr    ::= "fn" Params ":" Type "do" Block "end"
WithIoExpr    ::= "with" "IO" ( IoStmt ( "\n" | ";" )+ )* Expr "end"
IoStmt        ::= LowerIdent "<-" Expr | Expr
RecordLit     ::= "{" LowerIdent "=" Expr ( "," LowerIdent "=" Expr )* "}"
RecordUpdate  ::= "{" Expr "with" LowerIdent "=" Expr ( "," LowerIdent "=" Expr )* "}"

BinOp         ::= "and" | "or" | "implies"
                | "=" | "!=" | "<" | "<=" | ">" | ">="       -- a != b  means  not (a = b)
                | "+" | "-" | "*" | "/" | "%" | "+%" | "-%" | "*%"
                | "&" | "|" | "^" | "<<" | ">>"
UnOp          ::= "not" | "-"
```

---

### Comparison Matrix: Gardel vs Bend vs Eiffel vs Rust vs Dafny

| Feature | Gardel | Bend | Eiffel | Rust | Dafny |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Purity & Total Determinism** | **100% Total & Pure** | Pure | Imperative & Object | Imperative & Functional | Total / Imperative |
| **Hoare-Style Contracts** | **Native First-Class** | None | Native (Runtime/Debug) | Crates / Assertions | Method contracts |
| **Formal Verification** | **Compile-Time SMT (Boogie/Z3)** | None | AutoProof (Research) | Prusti / Kani / Creusot | Compile-Time SMT (Z3) |
| **Runtime Assertion Overhead** | **0% (100% Erased)** | 0% | Configurable (5%–40%) | Configurable (`debug_assert`) | 0% (Erased) |
| **Parallel Execution Target** | **Automatic C11 + OpenMP (CPU). Bend output not yet run** | Automatic HVM/BendRT | Manual Threads (SCOOP) | Manual / Rayon / Tokio | Single-threaded C# / Go / C++ |
| **Numerics** | **64-bit Checked (`U64`/`I64`) + `MATH_INT`** | Native 64-bit / 24-bit | Machine Ints | Machine Ints (Debug panic) | Machine Ints + `int` |
| **AI Agent Diagnostics** | **Structured JSON (`--json-diagnostics`)** | Human text | Human text | Human text / JSON | Human text |
| **I/O Isolation** | **Linear `IO[A]` Firewall** | Monadic `IO` | Open everywhere | Open everywhere | Interactive / Method effects |

---

*Gardel is actively evolving. Join us in building software where correctness is proven, concurrency is free, and human and AI developers speak the exact same mathematical language.*

