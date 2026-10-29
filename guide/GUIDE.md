# The Gardel Programming Guide

Welcome to the definitive guide to **Gardel**, a pure, total, statically verified programming language that fuses **Eiffel's Design by Contract**, **Dafny's automated SMT verification**, and **Bend's massively parallel runtime**.

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
  - [Your First Program](#your-first-program)
  - [Running and Checking Code](#running-and-checking-code)
- [3. Primitives and 64-Bit Numerics](#3-primitives-and-64-bit-numerics)
  - [Machine Types vs Mathematical Integers](#machine-types-vs-mathematical-integers)
  - [Checked Operators & Overflow Proofs](#checked-operators--overflow-proofs)
  - [Wrapping and Bitwise Operators](#wrapping-and-bitwise-operators)
  - [Deterministic Floating Point (`F64`)](#deterministic-floating-point-f64)
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
  - [Equivalence Relations (`EQ`) vs SMT Identity (`=`)](#equivalence-relations-eq-vs-smt-identity-)
  - [Total Orders (`ORD`)](#total-orders-ord)
- [13. The Linear IO Firewall](#13-the-linear-io-firewall)
  - [Why Pure Modules Forbid Effects](#why-pure-modules-forbid-effects)
  - [Linear `IO[A]` Actions and `with IO`](#linear-ioa-actions-and-with-io)
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
- [16. Compiler CLI Cheatsheet & Grammar Reference](#16-compiler-cli-cheatsheet--grammar-reference)
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
  [ Eiffel Heritage ]         [ Dafny Engine ]            [ Bend Backend ]
  • Design by Contract        • Automated SMT (Z3)        • Automatic Fork-Join
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
- Contracts are translated into mathematical first-order logic formulas discharged by the **Boogie** and **Z3** automated theorem provers during compilation.
- Once verified, contracts are **completely erased**. The resulting C11 / Bend code has zero runtime assertion checks, zero overhead, and maximum speed.

---

## 2. Getting Started: Hello World

### Your First Program

Programs that interact with the outside world (printing to standard output, reading files) are defined inside a `program` module. The entry point is `main`, which returns an `IO` action:

```eiffel
program HELLO

feature {ANY}

  main(args: LIST[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print("Hello, world! Welcome to Gardel.\n")
        IO.exit(0)
      end
    end

end
```

### Running and Checking Code

The Gardel compiler is `gardelc`:

```bash
# Verify all contracts and proofs statically without executing
gardelc check hello.gdl

# Compile, verify, and run using the Bend v2 interpreter / BendRT runtime
gardelc run hello.gdl

# Compile directly to portable C11
gardelc emit-c hello.gdl -o hello.c
gcc -O3 hello.c -o hello

# Compile directly to Bend v2 (for CPU / CUDA / Metal execution)
gardelc emit-bend hello.gdl -o hello.bend
bend run-c hello.bend     # Multi-core CPU
bend run-cu hello.bend    # NVIDIA CUDA GPU
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

### Equivalence Relations (`EQ`) vs SMT Identity (`=`)

- `=` in contracts represents built-in **structural Leibniz equality** in SMT.
- The `EQ` concept defines **behavioral equivalence** (`equals(other)`). This allows non-canonical representations (such as quotient types, rational numbers `2/4 == 1/2`, or unordered sets) to conform soundly.
- The subconcept `CANONICAL_EQ` guarantees that behavioral equality coincides with structural term identity (`a.equals(b) = (a = b)`).

### Total Orders (`ORD`)

The `ORD` concept refines `EQ` and requires irreflexivity, transitivity, and totality:

```eiffel
concept ORD refines EQ
  less_than(other: Current): BOOL

  law irreflexive(a: Current)
    ensure not a.less_than(a) end

  law transitive(a: Current, b: Current, c: Current)
    require a.less_than(b); b.less_than(c)
    ensure a.less_than(c) end

  law total(a: Current, b: Current)
    ensure a.less_than(b) or a.equals(b) or b.less_than(a) end
end
```

Generic modules like `SORTED_LIST[A: ORD]` can safely assume these laws as axioms during sorting and lookup proofs.

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

  main(args: LIST[STRING]): IO[EXIT_CODE]
    do
      with IO
        line <- IO.read_line
        IO.print("You said: " + line + "\n")
        IO.exit(0)
      end
    end

end
```

Inside `with IO ... end`:
- `x <- e`: Monadic bind. Evaluates an IO action $e$ of type `IO[T]` and binds the result $x : T$.
- `e`: Statement step. Evaluates an action of type `IO[UNIT]` for its effect.
- The trailing expression must return `IO[R]`.

### Boundary Ingestion: Untrusted Data to Verified ADTs

External data (network packets, command line inputs, disk files) is inherently untrusted.

Untrusted data cannot be cast directly into an invariant-carrying ADT. It must pass through a **boundary constructor** returning `OPTION[T]` or `RESULT[T, E]`:

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
1. **Preconditions of callees**: Caller must satisfy callee's `require`.
2. **Postconditions**: Routine body must establish its `ensure`.
3. **Invariants**: Every constructed value must satisfy its `invariant`.
4. **Numerics**: Arithmetic operations must not overflow or divide by zero.
5. **Termination**: Recursive calls must decrease the `variant`.

### The AI Autonomous Repair Loop (`--json-diagnostics`)

When verification fails, run `gardelc --json-diagnostics` to receive structured, machine-actionable JSON:

```bash
gardelc check stack.gdl --json-diagnostics
```

```json
{
  "status": "error",
  "diagnostics": [
    {
      "file": "stack.gdl",
      "line": 42,
      "column": 9,
      "kind": "postcondition_violation",
      "contract_tag": "count_incremented",
      "formula": "Result.count = Current.count + 1",
      "path_conditions": [
        "Current.count < Current.capacity",
        "Current.size = Current.count"
      ],
      "counterexample": {
        "Current": { "size": 0, "limit": 10, "items": [] },
        "Result": { "size": 0, "limit": 10, "items": ["item_1"] }
      },
      "hint": "Size field was not updated in record construction."
    }
  ]
}
```

### Reading Counterexample Models

The counterexample gives the exact input state where the theorem fails:
- Here, `Current.size` was `0`, but `Result.size` remained `0` after push!
- The bug is immediately obvious: `{ Current with items = items.prepend(x) }` forgot to update `size = Current.size + 1`.

### Repair Patterns for LLMs

When repairing failing Gardel code:

| Diagnostic Failure | Common Root Cause | Recommended AI Fix |
| :--- | :--- | :--- |
| `overflow_add` | Unchecked addition `a + b` | Add `require a <= NAT.max - b` or rewrite using subtraction `diff = hi - lo` |
| `not_decreasing` | Recursive `variant` not shrinking | Check recursive arguments; verify decreasing metric (e.g., `variant hi - lo` where `mid = lo + (hi - lo) / 2`) |
| `invariant_violation` | Record literal misses constraint | Insert a `check` statement to prove intermediate lemma, or add a guard `if` |
| `precondition_violation` | Callee requires condition | Wrap callee in a branch `if precondition_test then ... else ...` |
| `postcondition_violation` | Result formula does not match | Inspect counterexample model values to spot omitted field updates |

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

Demonstrating algebraic concepts, total orders (`ORD`), and structural induction proofs:

```eiffel
concept EQ
  equals(other: Current): BOOL
  law agrees_with_structural(a: Current, b: Current)
    ensure sound: a.equals(b) = (a = b) end
end

concept ORD refines EQ
  less_than(other: Current): BOOL

  less_or_equal(other: Current): BOOL
    do Current.less_than(other) or Current.equals(other) end

  law irreflexive(a: Current)
    ensure not a.less_than(a) end

  law transitive(a: Current, b: Current, c: Current)
    require a.less_than(b); b.less_than(c)
    ensure a.less_than(c) end

  law total(a: Current, b: Current)
    ensure a.less_than(b) or a = b or b.less_than(a) end
end

module PEANO satisfies ORD

representation {ANY}
  | Zero
  | Succ(pred: PEANO)

feature {ANY}

  equals(other: PEANO): BOOL
    variant Current
    do
      match Current
        case Zero then
          match other
            case Zero    then True
            case Succ(_) then False
          end
        case Succ(p) then
          match other
            case Zero    then False
            case Succ(q) then p.equals(q)
          end
      end
    ensure
      structural: Result = (Current = other)
    end

  less_than(other: PEANO): BOOL
    variant Current
    do
      match Current
        case Zero then
          match other
            case Zero    then False
            case Succ(_) then True
          end
        case Succ(p) then
          match other
            case Zero    then False
            case Succ(q) then p.less_than(q)
          end
      end
    end

specify {ORD}

  law agrees_with_structural(a: PEANO, b: PEANO)
    variant a
    do
      match a
        case Zero then True
        case Succ(p) then
          match b
            case Zero then True
            case Succ(q) then agrees_with_structural(p, q)
          end
      end
    end

end
```

---

## 16. Compiler CLI Cheatsheet & Grammar Reference

### CLI Commands

```bash
# Typecheck and verify all contracts statically
gardelc check <file.gdl>

# Run typechecker with structured JSON diagnostics for AI agents
gardelc check <file.gdl> --json-diagnostics

# Audit all 'assume' blocks in the project
gardelc --audit

# Reject any 'assume' blocks (strict verification mode)
gardelc check <file.gdl> --strict

# Inspect public contract interface of a module (like Eiffel 'short')
gardelc --inspect <file.gdl>

# Execute program via the Bend v2 interpreter
gardelc run <file.gdl>

# Transpile to portable C11
gardelc emit-c <file.gdl> -o output.c

# Transpile to Bend v2 (for CPU / CUDA / Metal execution)
gardelc emit-bend <file.gdl> -o output.bend
```

### Formal Grammar (EBNF Summary)

```ebnf
File          ::= GroupDecl? ( ModuleDecl | FunctionsDecl | ProgramDecl | ConceptDecl )+

ModuleDecl    ::= "module" UpperIdent TypeParams? Satisfies?
                    ( RepresentationSection | CreateSection
                    | FeatureSection | SpecifySection )*
                    InvariantBlock?
                  "end"

FunctionsDecl ::= "functions" UpperIdent ( FeatureSection | SpecifySection )* "end"
ProgramDecl   ::= "program" UpperIdent FeatureSection* "end"
ConceptDecl   ::= "concept" UpperIdent TypeParams? Refines?
                    ( Signature | RoutineDecl | LawDecl )*
                  "end"

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

