# Detailed Implementation Plan — Step 1: Remediate `stdlib/stdlib.gdl` Foundations

**Date:** October 28, 2026  
**Completed:** October 29, 2026  
**Status:** ✅ **COMPLETED** (All milestones M1.1–M1.4 verified with 0 errors via `gardelc` Boogie/Z3 backend)  
**Reference Document:** [core_library_vs_functional_ideas_review.md](core_library_vs_functional_ideas_review.md)  
**Target Files:**
- `../stdlib/stdlib.gdl`
- `../gardelc/stdlib/stdlib.gdl`
- `../gardelc/src/main.rs` (`phase4_stdlib_source()` and associated stdlib verification tests)
- `../spec/gardel_language_specification_draft_v0_5.md` (§3, §10.5, §16)

---

## 1. Objectives & Guiding Principles

The objective of Step 1 is to ensure that the foundational data structures in Gardel's standard library (`SEQ`, `BIGNAT`, and `STRING`) have sound mathematical semantics and strictly align with the v0.4 specification before we add higher-order functional combinators or algebraic laws in Steps 2–4.

1. **Exact Specification Compliance:**
   - `BIGNAT`: Model must represent the mathematical natural number in `MATH_INT`, not the number of limbs.
   - `SEQ[A]`: Eliminate silent length degradation (`else ll`). Conform with §10.5: balanced binary tree representation with explicit stored length or clean induction.
   - `STRING`: Unambiguous byte sequence abstraction with valid UTF-8 word bounds and character query semantics.
2. **Zero Verification Regressions:** All changes must verify with 0 errors via `gardelc` (using Boogie and Z3) and keep all unit tests passing (`cargo test --manifest-path gardelc/Cargo.toml`).
3. **Single Source of Truth:** Keep `stdlib/stdlib.gdl` and `gardelc/src/main.rs:phase4_stdlib_source()` in exact lockstep.

---

## 2. Work Breakdown Structure

### Task 1.1: Remediate `BIGNAT` (Arbitrary-Precision Unsigned Integer)

#### Root Problems in Current `BIGNAT`
- **Model Inaccuracy:** `model` currently returns `Current.size`, which is the count of 64-bit limbs (e.g. `BIGNAT.from_u64(42).model` evaluates to `1`, not `42`). Contracts specifying arithmetic ($a + b = c$) cannot be expressed.
- **Incomplete Domain:** `from_u64(x)` has `require non_zero: x > 0`, artificially forbidding conversion of `0: U64`.
- **Radix & Limb Model:** Needs a recursive ghost polynomial evaluation function mapping limbs to `MATH_INT`.

#### Design & Specification
- **Radix:** Base $B = 2^{64}$.
- **Storage:** `limbs: LIST[U64]` in little-endian order (least-significant limb first).
- **Ghost Polynomial Evaluation:**
  In `specify {ANY}`, define a recursive helper:
  $$\text{poly}(limbs) = \begin{cases} 0 & \text{if Empty} \\ h + B \times \text{poly}(t) & \text{if Cons}(h, t) \end{cases}$$
- **Model Definition:**
  ```eiffel
  specify {ANY}
    value_of(ls: LIST[U64]): MATH_INT
      variant ls
      do
        match ls
          case Empty then 0
          case Cons(h, t) then h + 18446744073709551616 * value_of(t)
        end
      end

    model: MATH_INT
      do
        value_of(Current.limbs)
      end
  ```
- **Constructor Updates:**
  - `zero: BIGNAT`: `ensure value_is_zero: Result.model = 0` and `zero_limbs: Result.limb_count = 0`.
  - `from_u64(x: U64): BIGNAT`: Allow $x \ge 0$. If $x = 0$, return empty limbs (or single limb 0, respecting canonical zero invariant); for $x > 0$, single limb with `ensure val: Result.model = x`.
  - `push_high_limb(limb: U64)`: Define how adding a limb updates `Result.model`.
- **Canonical Representation Invariant:**
  - `size_matches: size = limbs.length`
  - `bounded_limbs: size <= 1024`

---

### Task 1.2: Remediate `SEQ[A]` (Balanced Binary Sequence)

#### Root Problems in Current `SEQ[A]`
- **Length Overflow Fallback:**
  ```eiffel
  if ll <= 9223372036854775807 and rl <= 9223372036854775807 then
    ll + rl
  else
    ll   -- <--- SILENT SEMANTIC VIOLATION!
  end
  ```
  If `rl` exceeds $2^{63}-1$, returning `ll` is mathematically invalid and breaks `Result = Current.model.length`.
- **Structural Alignment with Spec v0.4 §10.5:**
  Spec v0.4 defines `SEQ[A]` as *"a balanced binary tree with stored `U64` length and a ghost `model: LIST[A]`"*.
  Currently, `Concat(left: SEQ[A], right: SEQ[A])` has no stored length, forcing an $O(N)$ traversal with fuel unfolding on every `.length` query.

#### Design & Specification
- **Representation Option Selection:**
  We evaluate two architectural paths:
  - **Path A (Stored Length in Constructor, Spec §10.5 Strict):**
    ```eiffel
    representation {ANY}
      | Empty
      | Leaf(value: A)
      | Concat(left: SEQ[A], right: SEQ[A], len: NAT)
    ```
    - Invariant: for `Concat(l, r, len)`, `len = l.length + r.length` and `len <= NAT.max`.
    - `concat(other: SEQ[A])` requires `no_overflow: Current.length <= NAT.max - other.length`.
    - `length: NAT` is $O(1)$: returns `match Current case Empty then 0 case Leaf(_) then 1 case Concat(_, _, len) then len end`.
  - **Path B (Inductive Length with Contract Bounding):**
    Keep binary `Concat(left, right)`, but eliminate the `ll <= 9223372036854775807` branch by proving inductive length preservation directly against `Current.model.length` with `NAT` addition checked against `NAT.max`.
  
  > **Recommended Decision: Path A.** It directly fulfills Spec v0.4 §10.5, delivers $O(1)$ length for parallel scheduling, and guarantees that length calculation never overflows or degrades silently.
- **Model Correspondence:**
  - `empty`: `ensure Result.model = LIST.empty and Result.length = 0`
  - `singleton(x)`: `ensure Result.model = LIST.empty.prepend(x) and Result.length = 1`
  - `concat(other)`:
    ```eiffel
    concat(other: SEQ[A]): SEQ[A]
      require
        fits: Current.length <= 18446744073709551615 - other.length
      do
        Concat(Current, other, Current.length + other.length)
      ensure
        len_sum: Result.length = Current.length + other.length
        model_concat: Result.model = Current.model.append(other.model)
      end
    ```

---

### Task 1.3: Clarify & Remediate `STRING` (UTF-8 Byte Buffer)

#### Root Problems in Current `STRING`
- **Ambiguous Element Semantics:** Items are stored as `LIST[U64]`, but `push_utf8_word(w: U64)` does not constrain `w < 256` (if representing bytes) or scalar values (if representing Unicode code points).
- **Missing Fundamental Queries:** Lacks equality (`equals`), concatenation (`append`/`concat`), and clear distinction between `byte_length` and character count.
- **Spec v0.4 Alignment (§16):** Described as *"UTF-8 byte-buffer backed by `SEQ[U64]` with ghost `LIST[CHAR]`"*.

#### Design & Specification
- **Representation:**
  ```eiffel
  module STRING
  representation {NONE}
    bytes: LIST[U64]
    size: NAT
    limit: NAT
  ```
- **Byte Invariant:**
  - `valid_bytes: forall i: NAT. i < bytes.length implies bytes.get(i) < 256` (or explicit byte bounds on items).
- **Core Operations:**
  - `push_byte(b: U64)` requiring `is_byte: b < 256` and `has_room: Current.byte_length < Current.capacity`.
  - `byte_length: NAT` returning `Current.size`.
  - `is_empty: BOOL` returning `Current.size = 0`.
- **Model:**
  - Ghost model `chars: LIST[CHAR]` (or scalar value list) representing decoded characters, alongside `bytes: LIST[U64]`.

---

## 3. Verification & Testing Strategy

1. **Step-by-step SMT Verification:**
   - Test each module individually with `gardelc` Boogie/Z3 backend.
   - Verify that all preconditions, postconditions, and termination variants strictly discharge without needing `--bounded-check`.
2. **Compiler Synchronization:**
   - Update `phase4_stdlib_source()` in `gardelc/src/main.rs`.
   - Update `stdlib/stdlib.gdl`.
   - Run `cargo test --manifest-path gardelc/Cargo.toml` to ensure all 27 unit tests (especially `test_phase4_stdlib_package_verifies_all_modules`) pass cleanly.
3. **AST Lowering & C11 Emission Verification:**
   - Verify that C11 generation compiles cleanly for the updated `SEQ` and `BIGNAT` representations under Clang/GCC.

---

## 4. Execution Sequence & Milestones

| Milestone | Target | Description | Verification Criterion | Status |
| :--- | :--- | :--- | :--- | :---: |
| **M1.1** | `SEQ[A]` | Update representation with stored length; remove fallback; establish `model_concat` contract. | `gardelc` verifies `SEQ[A]` with 0 errors; length is $O(1)$. | ✅ **Done** |
| **M1.2** | `BIGNAT` | Change `model` to `MATH_INT` polynomial value; update `zero`, `from_u64`, and `push_high_limb`. | `model` returns true value in contracts; all arithmetic contracts sound. | ✅ **Done** |
| **M1.3** | `STRING` | Constrain byte ranges; add `is_empty` and byte invariants; clarify `LIST[CHAR]` ghost model. | Invariants prove at all construction sites. | ✅ **Done** |
| **M1.4** | Sync & Test | Update `stdlib/stdlib.gdl` and `gardelc/src/main.rs:phase4_stdlib_source()`. | `cargo test` passes 100% (all 27+ tests). | ✅ **Done** |

---

## 5. Implementation & Verification Report (October 29, 2026)

### 5.1 Standard Library Enhancements
- **`SEQ[A]`**:
  - Representation updated to `Concat(left: SEQ[A], right: SEQ[A], len: NAT)`.
  - `length: NAT` is $O(1)$ directly returning stored `ln`.
  - Removed silent `ll <= 9223372036854775807` length fallback.
  - Added `is_empty: BOOL` query.
  - Proved contracts: `empty_model: Result.model = LIST.empty`, `unit_model: Result.model = LIST.empty.prepend(x)`, and `model_concat: Result.model = Current.model.append(other.model)`.
- **`BIGNAT`**:
  - Implemented Horner polynomial evaluation `value_of(ls: LIST[U64]): MATH_INT` with radix $B = 2^{64}$ (`18446744073709551616`) and variant `ls.length`.
  - `model: MATH_INT` reveals `value_of` and returns `value_of(Current.limbs)`.
  - `zero` ensures `value_is_zero: Result.model = 0` and `zero_limbs: Result.limb_count = 0`.
  - `from_u64(x: U64)` accepts all $x \ge 0$, ensuring `val: Result.model = x` and `count_bounded: Result.limb_count <= 1`.
  - `push_high_limb(limb: U64)` ensures `model_updated: Result.model = limb + 18446744073709551616 * Current.model`.
- **`STRING`**:
  - Representation standardized to `bytes: LIST[U64]`, `size: NAT`, `limit: NAT`.
  - Added recursive ghost validator `all_bytes_valid(bs: LIST[U64]): BOOL` ensuring every byte is $< 256$.
  - Added class invariant `valid_bytes: all_bytes_valid(bytes)`.
  - Decoupled `byte_model: LIST[U64]` from `chars: LIST[CHAR]`.
  - Added `push_byte(b: U64)` requiring `is_byte: b < 256` and ensuring `non_empty: not Result.is_empty` alongside `push_utf8_word`.

### 5.2 Compiler (`gardelc`) Adjustments
1. **Extended Integer Lexing / AST to `u128`**:
   - `Literal::Int(u128)`, `Expr::Int(u128)`, and `Token::Int(u128)` allow representing the mathematical radix $2^{64} = 18446744073709551616$ in specification expressions.
2. **Ghost `MATH_INT` Arithmetic Overflow Exemption**:
   - In `emit_boogie_expr_obligations`, skipped emitting 64-bit machine `in_u64` assertions for `+`, `-`, and `*` operations in routines returning `MATH_INT`, preserving mathematical semantics per Spec v0.4 §3.
3. **Boogie Positive-Fuel Stabilization & Receiver-Independence**:
   - Added positive-fuel stabilization axioms (`fn_r(SuccFuel(SuccFuel(f)), ...) == fn_r(SuccFuel(f), ...)`) ensuring recursive constructor unfoldings align with caller fuel.
   - Added receiver-independence axioms for pure helper routines that do not reference `Current` or representation fields (`routine_uses_current`).
4. **Symbolic Evaluator Support**:
   - Updated `eval_expr_symbolic` to pass current receiver context on `Expr::Call` and prevent false 64-bit overflow on operands $\ge 2^{64}$.

### 5.3 Verification Results
- `cargo test --manifest-path gardelc/Cargo.toml`: **27/27 tests passed cleanly**.
- `gardelc stdlib/stdlib.gdl --strict`: Verified with **0 errors, 0 warnings** across `SEQ`, `BIGNAT`, `STRING`, and `OPTION`. Emitted valid Boogie (`.bpl`), Dafny (`.dfy`), Bend v2 (`.bend`), and C11 (`.c`) artifacts.
- `gardelc examples/greeter.gdl --strict`: Verified with **0 errors, 0 warnings** and successfully compiled native binary.

