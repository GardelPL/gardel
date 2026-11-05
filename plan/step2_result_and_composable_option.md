# Detailed Implementation Plan — Step 2: Introduce `RESULT[E, A]` and Composable `OPTION[A]`

**Date:** October 29, 2026  
**Status:** ✅ **COMPLETED** (All milestones verified, 0 errors, 28 tests passing)  
**Reference Documents:**
- [core_library_vs_functional_ideas_review.md](core_library_vs_functional_ideas_review.md) (§2.1, §3 Workstream 2, §4 Step 2)
- [core_library.md](core_library.md) (§2 `OPTION[A]`, §3 `RESULT[E, A]`, §8 Law Catalogue)
- [functional_ideas.md](functional_ideas.md) (§2 Priority 1: Algebraic Laws in `specify`)
- [step1_remediate_stdlib_foundations.md](step1_remediate_stdlib_foundations.md)
**Target Files:**
- `../stdlib/stdlib.gdl`
- `../gardelc/stdlib/stdlib.gdl` (if present) / `../gardelc/src/main.rs` (`phase4_stdlib_source()` and stdlib verification unit tests)
- `../spec/gardel_language_specification_draft_v0_5.md` (§5, §7.4, §8, §11.4, §16)

---

## 1. Objectives & Guiding Principles

With Step 1 complete (`SEQ[A]`, `BIGNAT`, and `STRING` mathematically sound and verified with 0 errors), Step 2 focuses on **completing the foundational absence and failure abstractions** in Gardel's core library (`OPTION[A]` and `RESULT[E, A]`).

1. **Fulfill Specification v0.4 Boundary Validation Requirements (§7.4, §11.4, §16):**
   - Spec v0.4 mandates that untrusted external data at the `IO` or module boundary enters invariant-carrying ADTs only through checked constructors returning `OPTION` or `RESULT`. Introducing `RESULT[E, A]` closes the last remaining P0 gap between `stdlib.gdl` and Spec v0.4.
2. **Composable Total Pipelines Over Partial Unpacking:**
   - Currently, `OPTION[A]` only provides `is_some` and the partial query `get` (`require present: Current.is_some`), forcing callers into manual branching. Adding `is_none`, `unwrap_or`, `map`, and `and_then` to both `OPTION[A]` and `RESULT[E, A]` (plus `map_error` on `RESULT`) enables total, failure-propagating functional pipelines.
3. **First-Order Verifiable Contracts & Algebraic Laws:**
   - Following the resolution in [core_library_vs_functional_ideas_review.md](core_library_vs_functional_ideas_review.md#L41-L50), higher-order combinators (`map`, `and_then`, `map_error`) and their algebraic laws (identity, composition, and absence/error propagation) must be stated **pointwise** and in first-order SMT terms (`Result = Some(f(v))`, `law map_none`, `law map_compose`) so Boogie and Z3 discharge them deterministically without higher-order function extensionality fragility.
4. **Zero Verification Regressions & Single Source of Truth:**
   - All modules and laws in `stdlib/stdlib.gdl` and `gardelc/src/main.rs:phase4_stdlib_source()` must verify with **0 errors and 0 warnings** under `gardelc --strict` and pass all `cargo test` suites.

---

## 2. Work Breakdown Structure

### Task 2.1: Extend `OPTION[A]` with Composable Operations & Algebraic Laws

#### Current Baseline Limitations
In [stdlib.gdl](../stdlib/stdlib.gdl#L3-L28), `OPTION[A]` defines only:
- `| None` and `| Some(value: A)`
- `is_some: BOOL`
- `get: A` (with `require present: Current.is_some`)

It lacks a negation query (`is_none`), a total default extractor (`unwrap_or`), functor/monad combinators (`map`, `and_then`), postconditions on `is_some`/`get`, and `specify` algebraic laws.

#### Design & Specification for `OPTION[A]`

1. **Strengthen Existing Accessors:**
   - `is_some: BOOL`:
     - Add postcondition `ensure def: Result = (Current != None)`.
   - `is_none: BOOL`:
     - Returns `not Current.is_some` (or matches `None -> True`, `Some(_) -> False`).
     - Postcondition: `ensure def: Result = (not Current.is_some)` and `is_empty_variant: Result = (Current = None)`.
   - `get: A`:
     - Precondition: `require present: Current.is_some`.
     - Postcondition: `ensure matches_some: Current = Some(Result)`.

2. **Add Total Default & Higher-Order Combinators:**
   - `unwrap_or(default: A): A`:
     ```eiffel
     unwrap_or(default: A): A
       do
         match Current
           case None    then default
           case Some(v) then v
         end
       ensure
         when_some: Current.is_some implies Result = Current.get
         when_none: Current.is_none implies Result = default
       end
     ```
   - `map[B](f: A -> B): OPTION[B]`:
     ```eiffel
     map[B](f: A -> B): OPTION[B]
       do
         match Current
           case None    then None
           case Some(v) then Some(f(v))
         end
       ensure
         preserves_some: Result.is_some = Current.is_some
         preserves_none: Result.is_none = Current.is_none
         mapped_val: Current.is_some implies Result = Some(f(Current.get))
       end
     ```
   - `and_then[B](f: A -> OPTION[B]): OPTION[B]`:
     ```eiffel
     and_then[B](f: A -> OPTION[B]): OPTION[B]
       do
         match Current
           case None    then None
           case Some(v) then f(v)
         end
       ensure
         none_propagates: Current.is_none implies Result.is_none
         some_chains: Current.is_some implies Result = f(Current.get)
       end
     ```

3. **Algebraic Laws in `specify {ANY}`:**
   - `law none_is_not_some(opt: OPTION[A])`: `ensure disjoint: opt.is_none = (not opt.is_some)`
   - `law unwrap_or_some(v: A, default: A)`: `ensure extracts: Some(v).unwrap_or(default) = v`
   - `law unwrap_or_none(default: A)`: `ensure defaults: None.unwrap_or(default) = default`
   - `law map_preserves_none[B](f: A -> B)`: `ensure none_mapped: None.map(f) = None`
   - `law map_some[B](v: A, f: A -> B)`: `ensure some_mapped: Some(v).map(f) = Some(f(v))`
   - `law and_then_left_unit[B](v: A, f: A -> OPTION[B])`: `ensure left_id: Some(v).and_then(f) = f(v)`
   - `law and_then_none[B](f: A -> OPTION[B])`: `ensure absorbs: None.and_then(f) = None`

---

### Task 2.2: Introduce `module RESULT[E, A]`

#### Architectural Decisions
- **Group Declaration:** Update line 1 of `stdlib/stdlib.gdl` to:
  ```eiffel
  group { SEQ, BIGNAT, STRING, OPTION, RESULT }
  ```
- **Type Parameter Order (`RESULT[E, A]`):**
  - Follows the convention established in [core_library_vs_functional_ideas_review.md](core_library_vs_functional_ideas_review.md#L37) and [core_library.md](core_library.md#L110): error type `E` first, success value type `A` second, so curried/conceptual transformations on the success channel (`A`) align with standard functor/monad laws.
- **Variant Naming (`Error` and `Ok`):**
  - Representation variants:
    ```eiffel
    representation {ANY}
      | Error(err: E)
      | Ok(val: A)
    ```
  - Provide named constructors in `create {ANY}` (`ok(v: A): RESULT[E, A]` and `error(e: E): RESULT[E, A]`) in addition to direct variant construction (`Ok(v)`, `Error(e)`), giving callers both pattern-matching symmetry and explicit factory routines with `ensure` contracts.

#### Design & Specification for `RESULT[E, A]`

1. **Constructors (`create {ANY}`):**
   ```eiffel
   create {ANY}

     ok(v: A): RESULT[E, A]
       do
         Ok(v)
       ensure
         is_ok_res: Result.is_ok
         val_set: Result.get = v
       end

     error(e: E): RESULT[E, A]
       do
         Error(e)
       ensure
         is_err_res: Result.is_error
         err_set: Result.get_error = e
       end
   ```

2. **Queries & Checked Accessors (`feature {ANY}`):**
   - `is_ok: BOOL`:
     ```eiffel
     is_ok: BOOL
       do
         match Current
           case Error(_) then False
           case Ok(_)    then True
         end
       ensure
         def: Result = (match Current case Error(_) then False case Ok(_) then True end)
       end
     ```
   - `is_error: BOOL`:
     ```eiffel
     is_error: BOOL
       do
         match Current
           case Error(_) then True
           case Ok(_)    then False
         end
       ensure
         complement: Result = (not Current.is_ok)
       end
     ```
   - `get: A` (Checked value extraction):
     ```eiffel
     get: A
       require
         ok_present: Current.is_ok
       do
         match Current
           case Ok(v) then v
         end
       ensure
         matches_ok: Current = Ok(Result)
       end
     ```
   - `get_error: E` (Checked error extraction):
     ```eiffel
     get_error: E
       require
         err_present: Current.is_error
       do
         match Current
           case Error(e) then e
         end
       ensure
         matches_err: Current = Error(Result)
       end
     ```

3. **Total Fallbacks, Conversion & Combinators (`feature {ANY}`):**
   - `unwrap_or(default: A): A`:
     ```eiffel
     unwrap_or(default: A): A
       do
         match Current
           case Error(_) then default
           case Ok(v)    then v
         end
       ensure
         when_ok: Current.is_ok implies Result = Current.get
         when_err: Current.is_error implies Result = default
       end
     ```
   - `to_option: OPTION[A]`:
     - Bridges `RESULT[E, A]` to `OPTION[A]` by discarding the error payload when only presence/absence is needed:
     ```eiffel
     to_option: OPTION[A]
       do
         match Current
           case Error(_) then None
           case Ok(v)    then Some(v)
         end
       ensure
         preserves_status: Result.is_some = Current.is_ok
       end
     ```
   - `map[B](f: A -> B): RESULT[E, B]`:
     ```eiffel
     map[B](f: A -> B): RESULT[E, B]
       do
         match Current
           case Error(e) then Error(e)
           case Ok(v)    then Ok(f(v))
         end
       ensure
         preserves_ok: Result.is_ok = Current.is_ok
         preserves_err: Current.is_error implies Result.get_error = Current.get_error
         mapped_ok: Current.is_ok implies Result = Ok(f(Current.get))
       end
     ```
   - `map_error[F](f: E -> F): RESULT[F, A]`:
     ```eiffel
     map_error[F](f: E -> F): RESULT[F, A]
       do
         match Current
           case Error(e) then Error(f(e))
           case Ok(v)    then Ok(v)
         end
       ensure
         preserves_ok: Result.is_ok = Current.is_ok
         preserves_val: Current.is_ok implies Result.get = Current.get
         mapped_err: Current.is_error implies Result = Error(f(Current.get_error))
       end
     ```
   - `and_then[B](f: A -> RESULT[E, B]): RESULT[E, B]`:
     ```eiffel
     and_then[B](f: A -> RESULT[E, B]): RESULT[E, B]
       do
         match Current
           case Error(e) then Error(e)
           case Ok(v)    then f(v)
         end
       ensure
         err_propagates: Current.is_error implies (Result.is_error and Result.get_error = Current.get_error)
         ok_chains: Current.is_ok implies Result = f(Current.get)
       end
     ```

4. **Algebraic Laws in `specify {ANY}`:**
   - `law ok_not_error(r: RESULT[E, A])`: `ensure dichotomy: r.is_error = (not r.is_ok)`
   - `law map_ok[B](v: A, f: A -> B)`: `ensure ok_mapped: Ok(v).map(f) = Ok(f(v))`
   - `law map_preserves_error[B](e: E, f: A -> B)`: `ensure err_untouched: Error(e).map(f) = Error(e)`
   - `law map_error_err[F](e: E, f: E -> F)`: `ensure err_mapped: Error(e).map_error(f) = Error(f(e))`
   - `law map_error_preserves_ok[F](v: A, f: E -> F)`: `ensure ok_untouched: Ok(v).map_error(f) = Ok(v)`
   - `law and_then_ok[B](v: A, f: A -> RESULT[E, B])`: `ensure chain_ok: Ok(v).and_then(f) = f(v)`
   - `law and_then_error[B](e: E, f: A -> RESULT[E, B])`: `ensure chain_err: Error(e).and_then(f) = Error(e)`

---

## 3. Compiler (`gardelc`) Adjustments Required for Step 2

During our architectural audit of [gardelc/src/main.rs](../gardelc/src/main.rs), we identified four specific places where `gardelc`'s Boogie, Dafny, and symbolic verification backends assume routines only use module-level type parameters (`m.type_params`) and first-order arguments. Supporting `map[B]`, `and_then[B]`, and `map_error[F]` on `OPTION[A]` and `RESULT[E, A]` requires the following targeted enhancements in `gardelc`:

1. **Routine-Level Type Parameters (`r.type_params` & `law` type parameters):**
   - **Parser (`parse_law`):** Allow optional type parameters `law name[B, C](...)` on `LawDecl` (matching `RoutineDecl` at [gardelc/src/main.rs#L1371-L1380](../gardelc/src/main.rs#L1371-L1380) and [core_library_vs_functional_ideas_review.md](core_library_vs_functional_ideas_review.md#L47)).
   - **Boogie Emitter (`emit_boogie`, `emit_boogie_verify_routine`, `emit_boogie_verify_law`):** Merge `m.type_params` and `r.type_params` (or `law.type_params`) when emitting `<A, B>` type quantifiers on `function fn_<name><A, B>(...)`, its fuel/frame axioms, and `procedure Verify_<name><A, B>(...)`.
   - **Dafny Emitter (`emit_dafny_routine`, `emit_dafny_law`):** Emit combined `<A, B>` type parameters on polymorphic functions and lemmas.

2. **Polymorphic Type Application in `map_boogie_type`:**
   - **Current Issue ([gardelc/src/main.rs#L4706-L4708](../gardelc/src/main.rs#L4706-L4708)):**
     ```rust
     if ty == m.name || ty.starts_with(&format!("{}[", m.name)) {
         return map_boogie_type("Current", m);
     }
     ```
     When `OPTION[A].map[B]` returns `OPTION[B]`, `ty.starts_with("OPTION[")` forces the return type to `(OPTION A)` instead of `(OPTION B)`!
   - **Fix:** Only shortcut `ty == m.name || ty == "Current"` to `map_boogie_type("Current", m)`. When `ty` has explicit bracketed type arguments (`OPTION[B]`, `RESULT[E, B]`, `RESULT[F, A]`), let the general generic parser at lines 4727–4735 map each argument recursively to `(OPTION B)`, `(RESULT E B)`, `(RESULT F A)`.

3. **First-Class Function Types (`A -> B`) and Function Parameter Calls (`f(v)`) in Boogie & Symbolic Verifier:**
   - **Boogie Theory for `A -> B`:**
     - Declare `type Fn1 A B;` and `function Fn1_app<A, B>(f: Fn1 A B, x: A) returns (B);` in the Boogie prelude.
     - In `map_boogie_type`, translate `X -> Y` into `(Fn1 <map(X)> <map(Y)>)`.
     - In `emit_boogie_expr_with_strings`, when `Expr::Call(fname, args)` is invoked and `fname` is a parameter/local variable of function type (`A -> B`), emit `Fn1_app(fname, arg)` instead of `fn_fname(fuel, Current, arg)`.
   - **Cross-Module / Variant References (`to_option` in `RESULT`):**
     - When `RESULT[E, A]` references `OPTION[A]` and its constructors `None` / `Some(v)` (or vice versa), ensure `OPTION` is declared in `RESULT`'s Boogie context (or resolved from the group environment) so `to_option` verifies cleanly.
   - **Symbolic Evaluator (`sample_values_for_type` & `eval_expr_symbolic`):**
     - Support `GardelVal::Func` (or default deterministic test morphisms for `A -> B`, `A -> OPTION[B]`, `A -> RESULT[E, B]`) so the symbolic pre-check evaluates `f(v)` cleanly without returning `EvalFault::Unknown`.

---

## 4. Verification & Testing Strategy

1. **Module-by-Module Strict Verification:**
   - Run `gardelc stdlib/stdlib.gdl --strict` to verify that `OPTION[A]` and `RESULT[E, A]` (along with `SEQ[A]`, `BIGNAT`, and `STRING`) discharge all preconditions, postconditions, match exhaustiveness checks, and `specify` laws via Boogie/Z3 with 0 errors and 0 warnings.
2. **Negative / Soundness Tests:**
   - Add unit tests in `gardelc/src/main.rs` verifying that:
     - Calling `r.get` on `RESULT[E, A]` without proving `r.is_ok` fails verification with contract tag `ok_present` and produces a `Error(...)` counterexample in `--json-diagnostics`.
     - Calling `r.get_error` on `RESULT[E, A]` without proving `r.is_error` fails verification with contract tag `err_present` and produces an `Ok(...)` counterexample.
     - A broken law or postcondition on `OPTION.map` or `RESULT.and_then` is caught by Z3.
3. **Compiler & Codegen Synchronization:**
   - Synchronize `stdlib/stdlib.gdl` and `gardelc/src/main.rs:phase4_stdlib_source()`.
   - Run `cargo test --manifest-path gardelc/Cargo.toml` and confirm all existing + new Step 2 unit tests pass.

---

## 5. Execution Sequence & Milestones

| Milestone | Target | Description | Verification Criterion | Status |
| :--- | :--- | :--- | :--- | :---: |
| **M2.1** | `gardelc` Polymorphic & HOF Support | Support routine/law type parameters `[B]`, distinct generic instantiations (`OPTION[B]`, `RESULT[E, B]`), and `Fn1` function application in Boogie, Dafny, and symbolic verifier. | Polymorphic HOF routines verify in Boogie/Z3 with 0 errors. |  **Completed** |
| **M2.2** | `OPTION[A]` Combinators & Laws | Add `is_none`, `unwrap_or`, `map[B]`, `and_then[B]`, strengthened `is_some`/`get` contracts, and `specify` laws to `OPTION[A]`. | `gardelc` verifies `OPTION[A]` and all laws with 0 errors. |  **Completed** |
| **M2.3** | `RESULT[E, A]` Module & Laws | Add `module RESULT[E, A]` (`Error`, `Ok`, `ok`, `error`, `is_ok`, `is_error`, `get`, `get_error`, `unwrap_or`, `to_option`, `map`, `map_error`, `and_then`, and `specify` laws). | `gardelc` verifies `RESULT[E, A]` and all laws with 0 errors. |  **Completed** |
| **M2.4** | Sync & Regression Suite | Synchronize `stdlib/stdlib.gdl` with `phase4_stdlib_source()`, add positive/negative unit tests for `OPTION` and `RESULT`. | `cargo test` passes 100%; `gardelc stdlib/stdlib.gdl --strict` passes with 0 errors, 0 warnings. |  **Completed** |

---

## 6. Execution Summary & Verification Audit

**Executed on:** October 29, 2026  
**Final Status:** ✅ **100% Verified** (0 errors, 0 warnings across all 5 standard modules; 28 unit tests passing).

### 6.1 Architectural Discoveries & Compiler Remediation

During the execution of Step 2, several deep interactions between higher-order polymorphic combinators, Boogie's monomorphizer, and Z3 E-matching were discovered and resolved:

1. **Boogie Monomorphization Infinite Loop (`/typeEncoding:p`):**
   - *Issue:* Boogie's default type encoding (`/typeEncoding:m`, monomorphize) computes the transitive closure of type applications statically. Because `OPTION[A]` defines `map[B](f: A -> B): OPTION[B]` and `and_then[B](f: A -> OPTION[B]): OPTION[B]`, Boogie unifies `B` with `OPTION B`, causing an unbounded recursive monomorphization expansion `OPTION (OPTION (OPTION ...))` that consumed >2GB RAM and hung indefinitely before invoking Z3.
   - *Fix:* Added `/typeEncoding:p` (predicates encoding, standard in Dafny) to Boogie command line flags in `gardelc/src/main.rs`. Boogie now processes higher-order polymorphic types instantly in under 0.5s without unbounded expansion.

2. **Ambiguous Type Variables in Constructor Axioms:**
   - *Issue:* For sum types with multiple type parameters (such as `RESULT[E, A]`), the constructor `Ok(v: A)` does not mention `E` in its parameters. When Boogie emitted untyped constructor calls `Ok(val)` inside the projection and injection axioms, Boogie's type checker issued warnings (`Warning: type parameter E is ambiguous, instantiating to int`) and bound `E` to `int`. Consequently, polymorphic routines using `RESULT[E, A]` failed to trigger the constructor axioms.
   - *Fix:* Explicitly ascribed the return type on all constructor calls in the Boogie axioms: `(Ok(val) : (RESULT E A))`.

3. **Symmetric Disjointness Axioms for Directed E-Matching:**
   - *Issue:* Emitting disjointness axioms only in one direction (`for i < j ... is_variant_j(x) ==> !is_variant_i(x)`) created triggers that did not fire when constructing `Ok(v)` and attempting to verify that `is_Error` is false.
   - *Fix:* Emitted symmetric pairwise disjointness axioms (`for i ... for j ... if i != j`) so Z3 deduces both `is_Ok ==> !is_Error` and `is_Error ==> !is_Ok` regardless of constructor polarity.

4. **Inlined Refinement Invariants:**
   - *Issue:* Newly constructed `Result` values in pure functions could not prove their own variant invariants because `RESULT_Invariant` was emitted as an uninterpreted Boogie function.
   - *Fix:* Emitted refinement invariants with `{:inline}` (`function {:inline} {}_Invariant(...) returns (bool)`), enabling Boogie and Z3 to inline the invariant definitions automatically.

5. **Executable Compilation Guard:**
   - *Issue:* `gardelc` previously called `gcc -O3 -fopenmp` on all modules, which failed when compiling abstract generic libraries (such as `RESULT[E, A]`) lacking a `main()` entrypoint.
   - *Fix:* Restricted native binary C compilation to `module.kind == ModuleKind::Program`.

### 6.2 Standard Library Additions

- **`OPTION[A]`:**
  - Added queries: `is_none: BOOL`, `unwrap_or(default: A): A`.
  - Added combinators: `map[B](f: A -> B): OPTION[B]`, `and_then[B](f: A -> OPTION[B]): OPTION[B]`.
  - Added verified laws: `none_is_not_some`, `unwrap_or_some`, `unwrap_or_none`, `map_preserves_none`, `map_some`, `and_then_left_unit`, `and_then_none`.
- **`RESULT[E, A]`:**
  - Representation: `| Error(err: E) | Ok(val: A)`.
  - Constructors: `ok(v: A): RESULT[E, A]`, `error(e: E): RESULT[E, A]`.
  - Queries: `is_ok: BOOL`, `is_error: BOOL`.
  - Safe extractors: `get: A` (`require ok_present: Current.is_ok`), `get_error: E` (`require err_present: Current.is_error`).
  - Total fallbacks: `unwrap_or(default: A): A`, `to_option: OPTION[A]`.
  - Combinators: `map[B](f: A -> B): RESULT[E, B]`, `map_error[F](f: E -> F): RESULT[F, A]`, `and_then[B](f: A -> RESULT[E, B]): RESULT[E, B]`.
  - Verified laws: `ok_not_error`, `map_ok`, `map_preserves_error`, `map_error_err`, `map_error_preserves_ok`, `and_then_ok`, `and_then_error`.

### 6.3 Test & Verification Evidence

- `cargo test --manifest-path gardelc/Cargo.toml`:
  - 28 passing tests in 4.10s.
  - Added `test_step2_result_and_option_soundness_and_precondition_failures` validating that calling `get` on an `Error` value or `get_error` on an `Ok` value triggers precondition failures (`ok_present` / `err_present`).
- `gardelc stdlib/stdlib.gdl --strict`:
  - 0 errors, 0 warnings across `SEQ`, `BIGNAT`, `STRING`, `OPTION`, and `RESULT`.
- `spec/gardel_language_specification_draft_v0_5.md`:
  - Updated §7.4 with `try_make_result`.
  - Updated §16 roadmap locking `RESULT[E, A]`.

