# The Gardel Programming Guide

Welcome to the definitive guide to **Gardel**, a pure, total, statically verified programming language that fuses **Eiffel's Design by Contract**, **Dafny-style automated SMT verification** (carried out by Boogie and Z3), and **Bend's massively parallel runtime**.

This guide is written for two audiences:
1. **Software engineers** seeking mathematical certainty, zero-cost static contracts, and lock-free parallel execution on modern CPUs and GPUs.
2. **AI coding agents (LLMs)** generating, evaluating, and repairing high-integrity code via structured SMT diagnostics.

---

## Table of Contents

- [1. Philosophy and Mental Model](#1-philosophy-and-mental-model)
  - [The Triad: Eiffel + Dafny + Bend](#the-triad-eiffel--dafny--bend)
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
- [6. Eiffel-Style Design by Contract](#6-eiffel-style-design-by-contract)
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
- [10. Automatic Deterministic Parallelism](#10-automatic-deterministic-parallelism)
  - [Divide-and-Conquer Parallel Fork-Join](#divide-and-conquer-parallel-fork-join)
  - [Church-Rosser Confluence](#church-rosser-confluence)
  - [The `SEQ[A]` Balanced Tree Collection](#the-seqa-balanced-tree-collection)
  - [BendRT Runtime: CPU Cores, CUDA, and Apple Metal](#bendrt-runtime-cpu-cores-cuda-and-apple-metal)
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
  - [Boundary Ingestion: Untrusted Data to Verified ADTs](#boundary-ingestion-untrusted-data-to-verified-adts)
- [14. SMT Proofs & AI Agent Verification Workflow](#14-smt-proofs--ai-agent-verification-workflow)
  - [The Boogie / Z3 Verification Engine](#the-boogie--z3-verification-engine)
  - [The AI Autonomous Repair Loop (`--json-diagnostics`)](#the-ai-autonomous-repair-loop---json-diagnostics)
  - [Reading Counterexample Models](#reading-counterexample-models)
  - [Repair Patterns for LLMs](#repair-patterns-for-llms)
- [15. Complete Real-World Examples](#15-complete-real-world-examples)
  - [Example A: `BOUNDED_STACK[A]`](#example-a-bounded_stacka)
  - [Example B: Verified Parallel Tree Construction & Maximum](#example-b-verified-parallel-tree-construction--maximum)
  - [Example C: Peano Numbers with Formally Verified Total Ordering](#example-c-peano-numbers-with-formally-verified-total-ordering)
- [16. Projects and the Standard Prelude](#16-projects-and-the-standard-prelude)
  - [The Prelude Is Always There](#the-prelude-is-always-there)
  - [Multi-File Projects](#multi-file-projects)
  - [The Standard Library Sources (`stdlib/`)](#the-standard-library-sources-stdlib)
- [17. Compiler CLI Cheatsheet & Grammar Reference](#17-compiler-cli-cheatsheet--grammar-reference)
  - [CLI Commands](#cli-commands)
  - [Formal Grammar (EBNF Summary)](#formal-grammar-ebnf-summary)
  - [Comparison Matrix: Gardel vs Bend vs Eiffel vs Rust vs Dafny](#comparison-matrix-gardel-vs-bend-vs-eiffel-vs-rust-vs-dafny)

---

## 1. Philosophy and Mental Model

### The Triad: Eiffel + Dafny + Bend

Modern programming languages force developers into compromises:
- **Imperative languages (C, Rust, Go)** offer high performance and machine control, but concurrency introduces race conditions, deadlocks, and mutation hazards.
- **Verification languages (Dafny, Lean, Coq)** provide rock-solid formal proofs, but compiling them into high-performance, massively parallel binaries is often secondary.
- **Parallel languages (Bend / HVM)** enable effortless, lock-free parallel scaling across CPUs and GPUs, but lack integrated specification contracts and formal correctness proofs.

**Gardel synthesizes all three into a single, cohesive paradigm:**

```
                  ┌──────────────────────────────────────┐
                  │                GARDEL                │
                  └──────────────────┬───────────────────┘
                                     │
         ┌───────────────────────────┼───────────────────────────┐
         ▼                           ▼                           ▼
  [ Eiffel Heritage ]         [ Dafny Heritage ]          [ Bend Backend ]
  • Design by Contract        • SMT proofs (Boogie + Z3)  • Automatic Fork-Join
  • require / ensure          • Proofs erased at runtime  • Massive Parallelism
  • Class Invariants          • Refinement types          • CPU (C11), GPU (CUDA/Metal)
  • Uniform Access & Current  • Inductive ghost models    • Church-Rosser Confluence
```

### The Core Invariant: Total Purity and Determinism

In Gardel:
1. **Values are immutable.** There are no variables that change over time, no pointers, and no in-place mutation.
2. **All routines are total functions.** A routine cannot loop forever, crash with an unhandled exception, or return undefined. Given valid inputs meeting its `require` clause, it is mathematically proven to terminate and produce a result satisfying its `ensure` clause.
3. **Strict Church-Rosser confluence.** Because every expression is purely functional and total, evaluating independent expressions sequentially, interleaved, or in parallel on thousands of GPU threads produces the **exact same bit-for-bit result** for discrete types.

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
- **Bend / BendRT (Optional, for GPU / parallel interaction nets):** Install Bend v2 via `cargo install bend-lang` if you plan to execute `.bend` files on CUDA or Apple Metal.

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
# Output: test result: ok. 73 passed; 0 failed
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

# Run the emitted Bend v2 file (in build/):
bend run-c build/hello.bend     # Multi-core CPU
bend run-cu build/hello.bend    # NVIDIA CUDA GPU
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

Standard arithmetic operators (`+`, `-`, `*`, `/`, `%`) generate static proof obligations:
- For `a + b`, you must prove $a + b \le 2^{64} - 1$ (for `U64`).
- For `a - b`, you must prove $a - b \ge 0$ (for `U64`).
- For `a / b` and `a % b`, you must prove $b \ne 0$.

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
- Use wrapping operators: `+%`, `-%`, `*%`. These are total and defined modulo $2^{64}$.
- Use bitwise operators: `&`, `|`, `^`, `<<`, `>>`. Verified via Z3's `(_ BitVec 64)` bitvector theory. Shifts require proving `shift_amount < 64`.

### Deterministic Floating Point (`F64`)

Floating-point math in Gardel follows strict syntactic expression-tree evaluation order:
- Compiler re-association (such as `-ffast-math`) and uncoordinated fused multiply-add (`fma`) synthesis are prohibited.
- For discrete types (`NAT`, `INT`, `BOOL`, ADTs), Church-Rosser confluence guarantees exact bit-for-bit equivalence regardless of parallel thread scheduling.

### Text: `CHAR` and `STRING`

`CHAR` is a Unicode scalar value. `STRING` is a prelude module (`stdlib/text/string.gdl`) whose ghost model is `chars: LIST[CHAR]`.

> [!NOTE]
> The current `STRING` is the **ASCII bootstrap**: every byte is `< 128`. Full multi-byte UTF-8 arrives with Roadmap Task C.2 without changing the public `chars` contracts.

String literals are ordinary verified values. `"Hi"` is defined as a chain of verified prelude calls (`STRING.make_empty(2).push_ascii('i').push_ascii('H')`), so the verifier knows `"Hi".chars` and `"Hi".byte_length = 2` with no extra trust. The compiler enforces the bootstrap rules:

| Code | Rejected | Use instead |
| :--- | :--- | :--- |
| `E0303` | A literal with a non-ASCII character | ASCII text until Task C.2 |
| `E0302` / `E0304` | Malformed escapes / surrogate code points | Valid escapes |
| `E0311` | `=` with a `STRING` operand (`STRING` is not `CANONICAL_EQ`: two strings with different capacities are equal text) | `a.equals(b)` or `a.chars = b.chars` |
| `E0312` | `+` on `STRING` | Print the pieces with separate `IO.print` calls |
| `E0313` | String literals as `match` patterns | `if s.equals("...") then ... else ... end` |

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

## 6. Eiffel-Style Design by Contract

Contracts form the central nervous system of Gardel. Inspired by Bertrand Meyer's Eiffel, contracts express clear mathematical agreements between callers and suppliers.

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

1. **Contract Visibility**: Every symbol in a routine's `require` clause must be at least as visible as the routine itself. Callers cannot be asked to satisfy conditions they cannot see.
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

Tail-recursive calls are automatically lowered by `gardelc` into **zero-allocation native C `while` loops** and BendRT tail jumps.

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
// Emitted Bend / BendRT task fork:
left_max right_max = max_value(l) max_value(r)
```

Both branches run in parallel on separate CPU cores or GPU threads.

### Church-Rosser Confluence

Because functions have no side effects, the order in which parallel tasks finish has zero impact on the final result:
$$\text{Sequential Evaluation} \equiv \text{Parallel CPU Execution} \equiv \text{Massive CUDA/Metal GPU Execution}$$

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

Operations like `map`, `reduce`, and `filter` over `SEQ[A]` split work evenly across CPU task rings and GPU SIMD lanes.

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

### Lambda Syntax (`fn`)

Anonymous functions use the `fn` keyword:

```eiffel
let add_one = fn(x: NAT): NAT do x + 1 end
```

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

> [!NOTE]
> A module's laws are axioms only for its **clients**. While the module itself is verified, its laws are proof obligations, never assumptions, so a law cannot be used to prove itself.

### Conditional Conformance (`where`)

A generic module can conform to a concept only when its type arguments do:

```eiffel
module SEQ[A] satisfies EQ where A satisfies CANONICAL_EQ
module RESULT[E, A] satisfies CANONICAL_EQ where E satisfies CANONICAL_EQ, A satisfies CANONICAL_EQ
```

The constraint is enforced where the concept's routines are used. `SEQ.equals` compares elements structurally, which is behavioral equality only for canonical elements, so `SEQ[U64].equals` is accepted while `SEQ[STRING].equals` and `SEQ[SEQ[U64]].equals` are rejected with `E0314: unsatisfied_where_constraint`. Primitives satisfy `CANONICAL_EQ` and `ORD`, a type parameter satisfies its declared bound (`[A: CANONICAL_EQ]`), and a module type satisfies the concepts it lists when its own `where` constraints hold.

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

## 14. SMT Proofs & AI Agent Verification Workflow

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
| Termination Variant Violation / `variant_decrease` | A recursive call does not shrink the `variant` | Check the call's arguments; pick a metric that decreases on every call in the group |
| Termination Variant Violation / `variant_missing` | A routine in a recursive group has no `variant` | Add a `variant` to every routine of the group |
| Termination Variant Violation / `variant_non_negative` | An integer metric can go below 0 | Strengthen `require` or choose a `NAT` metric |
| `E0315: higher_order_recursion` | Recursion through a lambda or function value | Call the routine directly |
| Invariant Violation / invariant tag | Constructed value breaks the `invariant` | Insert a `check` to prove the intermediate lemma, or add a guard `if` |
| Precondition Violation / callee's `require` tag | Callee requires a condition the caller does not establish | Wrap the call in `if precondition_test then ... else ... end` |
| Postcondition Violation / `ensure` tag | Result does not match the claim | Inspect counterexample values to spot omitted field updates |
| `E0314: unsatisfied_where_constraint` | `equals` on e.g. `SEQ[STRING]` | Compare models (`a.model = b.model`) or use canonical elements |

---

## 15. Complete Real-World Examples

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
          -- Independent recursive calls fork onto BendRT parallel rings:
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

## 16. Projects and the Standard Prelude

### The Prelude Is Always There

Every program automatically sees the **Standard Prelude**:

| Kind | Names |
| :--- | :--- |
| Concepts | `EQ`, `CANONICAL_EQ`, `ORD` |
| Core types | `OPTION[A]`, `RESULT[E, A]`, `IO_ERROR` |
| Collections | `SEQ[A]`, `LIST_OPS` |
| Numbers | `BIGNAT` |
| Text | `STRING`, `STRING_ERROR` |
| Functions | `FUNCTION_OPS` |

The prelude is verified once, on its own (`gardelc --stdlib`). In user programs its contracts and laws are trusted facts, and calls into it must still establish its preconditions. Declaring a top-level name that the prelude already uses is an error (`E0101: duplicate_prelude_symbol`). Pass `--no-prelude` only to build a prelude-free program on purpose.

### Multi-File Projects

Give `gardelc` several files, a directory (searched recursively, skipping `build/` and `target/`), or a wildcard pattern, and they form **one project** with a flat module namespace:

```bash
gardelc src/                  # every .gdl file under src/
gardelc src/*.gdl --strict    # a wildcard pattern
gardelc a.gdl b.gdl           # explicit files
```

Rules:
- **One module per file**, named after it: `module BANK_ACCOUNT` lives in `bank_account.gdl`. Otherwise you get `W0201: file_module_name_mismatch`, which `--strict` turns into a failure.
- **No duplicates:** two files declaring the same module is `E0102`.
- **No cycles between files:** each file's proofs trust the contracts of the files it uses, so a cycle would let two modules prove each other. Cross-file cycles are rejected with `E0103: cyclic_module_dependency`; genuinely mutually dependent modules belong in one file.

### The Standard Library Sources (`stdlib/`)

The prelude's source code lives in the repository's `stdlib/` folder, one module or concept per file, and is embedded into `gardelc` when the compiler is built:

```text
stdlib/
├── concepts/      eq.gdl, canonical_eq.gdl, ord.gdl
├── core/          option.gdl, result.gdl, io_error.gdl
├── collections/   list_ops.gdl, seq.gdl
├── numeric/       bignat.gdl
├── text/          string.gdl, string_error.gdl
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

## 17. Compiler CLI Cheatsheet & Grammar Reference

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

# Verify the Standard Prelude sources (stdlib/) as one linked project
gardelc --stdlib --strict

# Run verification with structured JSON diagnostics for AI agents
gardelc <file.gdl> --json-diagnostics

# Audit all 'assume' blocks in the file or project
gardelc <file.gdl> --audit

# Reject any 'assume' blocks (strict verification mode)
gardelc <file.gdl> --strict

# Inspect public contract interface of a module (like Eiffel 'short')
gardelc --inspect <file.gdl>
gardelc --stdlib --inspect [MODULE]

# Unfold recursive definitions to depth N for the solver (default 2)
gardelc <file.gdl> --fuel <N>

# Compile without the Standard Prelude (bootstrap mode)
gardelc <file.gdl> --no-prelude

# Bounded symbolic checking instead of the Boogie/Z3 proof
gardelc <file.gdl> --bounded-check

# Compile, verify, and run immediately via the backend
gardelc <file.gdl> --run
```

A normal run reports five steps:

```text
[0/5] Standard Prelude injected (or bootstrap mode)
[1/5] Parsed / linked the input files
[2/5] Checked visibility, concept laws, IO boundary, and Boogie/Z3 verification -> errors and warnings
[3/5] Emitted Boogie VCs        -> build/<module>.bpl
[4/5] Emitted Bend v2 target    -> build/<module>.bend
[5/5] Emitted C11 runtime target -> build/<module>.c
```

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
ProgramDecl   ::= "program" UpperIdent FeatureSection* "end"
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
| **Design by Contract** | **Native First-Class** | None | Native (Runtime/Debug) | Crates / Assertions | Method contracts |
| **Formal Verification** | **Compile-Time SMT (Boogie/Z3)** | None | AutoProof (Research) | Prusti / Kani / Creusot | Compile-Time SMT (Z3) |
| **Runtime Assertion Overhead** | **0% (100% Erased)** | 0% | Configurable (5%–40%) | Configurable (`debug_assert`) | 0% (Erased) |
| **Parallel Execution Target** | **Automatic BendRT / C11 (CPU/CUDA/Metal)** | Automatic HVM/BendRT | Manual Threads (SCOOP) | Manual / Rayon / Tokio | Single-threaded C# / Go / C++ |
| **Numerics** | **64-bit Checked (`U64`/`I64`) + `MATH_INT`** | Native 64-bit / 24-bit | Machine Ints | Machine Ints (Debug panic) | Machine Ints + `int` |
| **AI Agent Diagnostics** | **Structured JSON (`--json-diagnostics`)** | Human text | Human text | Human text / JSON | Human text |
| **I/O Isolation** | **Linear `IO[A]` Firewall** | Monadic `IO` | Open everywhere | Open everywhere | Interactive / Method effects |

---

*Gardel is actively evolving. Join us in building software where correctness is proven, concurrency is free, and human and AI developers speak the exact same mathematical language.*

