# Detailed Implementation Plan — Step 3: Build the Verified Functional Core on `LIST` and `SEQ`

**Date:** October 30, 2026  
**Status:** ✅ **COMPLETED** (Verified with 0 errors, 0 warnings under `gardelc stdlib/stdlib.gdl --strict`; 31/31 unit tests passing)  
**Reference Documents:**
- [core_library_vs_functional_ideas_review.md](core_library_vs_functional_ideas_review.md) (§2.2, §3 Workstream 3, §4 Step 3)
- [core_library.md](core_library.md) (§4 `SEQ[A]`: establish the functional core, §6 Function composition, §8 Law Catalogue)
- [functional_ideas.md](functional_ideas.md) (§2 Priority 1 & 3, §3 Library combinators for `fold`, §4 Function composition, §7 ADT refinement proofs)
- [step1_remediate_stdlib_foundations.md](step1_remediate_stdlib_foundations.md)
- [step2_result_and_composable_option.md](step2_result_and_composable_option.md)
**Target Files:**
- `../stdlib/stdlib.gdl`
- `../gardelc/src/main.rs` (`phase4_stdlib_source()`, Boogie/Dafny/C11 emitters, and stdlib verification unit tests)
- `../spec/gardel_language_specification_draft_v0_4.md` (§8, §10.5, §16)

---

## 1. Objectives & Guiding Principles

With Step 1 (`SEQ`, `BIGNAT`, `STRING` foundations) and Step 2 (`RESULT[E, A]` and composable `OPTION[A]`) verified with 0 errors, Step 3 delivers **Gardel's Verified Functional Core** across mathematical lists (`LIST_OPS`), parallel balanced sequences (`SEQ[A]`), and higher-order function combinators (`FUNCTION_OPS`).

1. **Dual-Layer Collection Architecture (`LIST[A]` Specification Model vs. `SEQ[A]` Parallel Tree):**
   - As established in Spec v0.4 (§8, §10.5) and [functional_ideas.md](functional_ideas.md#L242-L254), `LIST[A]` is the canonical sequential/inductive specification model, whereas `SEQ[A]` is the parallel balanced binary tree representation (`Empty | Leaf(v) | Concat(l, r, len)`).
   - Every transformation and reduction on `SEQ[A]` (`map`, `filter`, `fold_left`, `fold_right`) must be connected to its corresponding operation in `LIST_OPS` via explicit model-refinement postconditions (`Result.model = LIST_OPS.map(Current.model, f)`, etc.), realizing Bird & Wadler Chapter 8 abstraction refinement proofs.
2. **Library Combinators Over Dedicated Syntax (§3, §10):**
   - Spec v0.4 deliberately removed the v0.2 `fold`/`unfold` keywords. Step 3 provides `fold_left` and `fold_right` as verified higher-order library routines on both `LIST_OPS` and `SEQ[A]`, preserving uniform recursion while enabling expressive reductions.
3. **Parallel Fork-Join Lowering on `SEQ[A]`:**
   - Divide-and-conquer operations on `SEQ[A]` (`map`, `filter`) must bind recursive subtrees via independent `let` bindings (`let l_res = l.map(f)` and `let r_res = r.map(f)`) so `gardelc` automatically lowers them into parallel fork-join tasks in Bend v2 and OpenMP/C11 (§10.2).
4. **Pointwise First-Order Specification of Higher-Order Combinators:**
   - Following the resolution in [core_library_vs_functional_ideas_review.md](core_library_vs_functional_ideas_review.md#L41-L50), `FUNCTION_OPS.compose` and its identity/associativity laws must be specified **pointwise** (`forall x: A. Result(x) = f(g(x))`) so Boogie and Z3 verify them deterministically without higher-order extensionality fragility.
5. **Zero Verification Regressions & Single Source of Truth:**
   - Keep `stdlib/stdlib.gdl` and `gardelc/src/main.rs:phase4_stdlib_source()` in exact lockstep, verifying all modules with **0 errors and 0 warnings** under `gardelc --strict`.

---

## 2. Work Breakdown Structure

### Task 3.1: Introduce `functions LIST_OPS` (Sequential Model Combinators & Homomorphism Laws)

#### Role in the Architecture
`LIST_OPS` provides the canonical recursive definitions of `map`, `filter`, `fold_left`, and `fold_right` over `LIST[A]`, along with structural homomorphism lemmas showing how each combinator distributes over `LIST.append`. These homomorphism laws are the mathematical bridge required by Z3 to discharge `SEQ[A]` refinement postconditions on `Concat(l, r, len)` nodes.

#### Design & Specification for `LIST_OPS`

1. **Core Combinators (`feature {ANY}`):**
   - `map[A, B](xs: LIST[A], f: A -> B): LIST[B]`:
     ```eiffel
     map[A, B](xs: LIST[A], f: A -> B): LIST[B]
       variant
         xs.length
       do
         if xs.length = 0 then
           LIST.empty
         else
           LIST_OPS.map(xs.tail, f).prepend(f(xs.head))
         end
       ensure
         same_length: Result.length = xs.length
         empty_case: xs.length = 0 implies Result = LIST.empty
         cons_case: xs.length > 0 implies Result = LIST_OPS.map(xs.tail, f).prepend(f(xs.head))
       end
     ```
   - `filter[A](xs: LIST[A], keep: A -> BOOL): LIST[A]`:
     ```eiffel
     filter[A](xs: LIST[A], keep: A -> BOOL): LIST[A]
       variant
         xs.length
       do
         if xs.length = 0 then
           LIST.empty
         else
           let rest = LIST_OPS.filter(xs.tail, keep)
           if keep(xs.head) then
             rest.prepend(xs.head)
           else
             rest
           end
         end
       ensure
         not_longer: Result.length <= xs.length
         empty_case: xs.length = 0 implies Result = LIST.empty
       end
     ```
   - `fold_left[A, B](xs: LIST[A], f: (acc: B, x: A) -> B, init: B): B`:
     ```eiffel
     fold_left[A, B](xs: LIST[A], f: (acc: B, x: A) -> B, init: B): B
       variant
         xs.length
       do
         if xs.length = 0 then
           init
         else
           LIST_OPS.fold_left(xs.tail, f, f(init, xs.head))
         end
       ensure
         empty_case: xs.length = 0 implies Result = init
         cons_case: xs.length > 0 implies Result = LIST_OPS.fold_left(xs.tail, f, f(init, xs.head))
       end
     ```
   - `fold_right[A, B](xs: LIST[A], f: (x: A, acc: B) -> B, init: B): B`:
     ```eiffel
     fold_right[A, B](xs: LIST[A], f: (x: A, acc: B) -> B, init: B): B
       variant
         xs.length
       do
         if xs.length = 0 then
           init
         else
           f(xs.head, LIST_OPS.fold_right(xs.tail, f, init))
         end
       ensure
         empty_case: xs.length = 0 implies Result = init
         cons_case: xs.length > 0 implies Result = f(xs.head, LIST_OPS.fold_right(xs.tail, f, init))
       end
     ```

2. **Algebraic & Distributivity Laws in `specify {ANY}`:**
   - `law map_empty[A, B](f: A -> B)`:
     `ensure is_empty: LIST_OPS.map(LIST.empty, f) = LIST.empty`
   - `law map_singleton[A, B](x: A, f: A -> B)`:
     `ensure is_single: LIST_OPS.map(LIST.empty.prepend(x), f) = LIST.empty.prepend(f(x))`
   - `law map_append[A, B](xs: LIST[A], ys: LIST[A], f: A -> B)`:
     `ensure distributes: LIST_OPS.map(xs.append(ys), f) = LIST_OPS.map(xs, f).append(LIST_OPS.map(ys, f))`
   - `law filter_append[A](xs: LIST[A], ys: LIST[A], keep: A -> BOOL)`:
     `ensure distributes: LIST_OPS.filter(xs.append(ys), keep) = LIST_OPS.filter(xs, keep).append(LIST_OPS.filter(ys, keep))`
   - `law fold_left_append[A, B](xs: LIST[A], ys: LIST[A], f: (acc: B, x: A) -> B, init: B)`:
     `ensure chains: LIST_OPS.fold_left(xs.append(ys), f, init) = LIST_OPS.fold_left(ys, f, LIST_OPS.fold_left(xs, f, init))`
   - `law fold_right_append[A, B](xs: LIST[A], ys: LIST[A], f: (x: A, acc: B) -> B, init: B)`:
     `ensure chains: LIST_OPS.fold_right(xs.append(ys), f, init) = LIST_OPS.fold_right(xs, f, LIST_OPS.fold_right(ys, f, init))`

---

### Task 3.2: Extend `SEQ[A]` with Verified Parallel Combinators & Model Refinement Proofs

#### Current Baseline in `SEQ[A]`
In [stdlib.gdl](../stdlib/stdlib.gdl#L116-L184), `SEQ[A]` already provides:
- Representation: `| Empty | Leaf(value: A) | Concat(left: SEQ[A], right: SEQ[A], len: NAT)`
- Ghost model: `model: LIST[A]`
- Constructors: `empty`, `singleton(x: A)`
- Features: `length: NAT`, `is_empty: BOOL`, and `concat(other: SEQ[A]): SEQ[A]` (with `ensure model_concat: Result.model = Current.model.append(other.model)`)

#### Strengthening `SEQ[A]` Length-Model Coherence
To ensure `filter` can safely concatenate filtered left and right subtrees without risking 64-bit `U64` addition overflow on `fl.length + fr.length`:
- Strengthen `SEQ[A]` with class invariant (or inductive `length` postcondition):
  `len_matches_model: Result.length = Current.model.length`
- Because `Current.length` is a `NAT` (`in_u64(Current.length)`), any sub-sequence produced by `filter` has `Result.length = Result.model.length <= Current.model.length = Current.length`. Thus, for `Concat(l, r, ln)`, `fl.length + fr.length <= l.length + r.length = ln <= 18446744073709551615`.

#### Design & Specification of New `SEQ[A]` Operations

1. **`map[B](f: A -> B): SEQ[B]` (Parallel Structure-Preserving Transformation):**
   - Preserves the exact binary tree topology and cached `ln` of `Concat(l, r, ln)` while transforming leaves in parallel via independent `let` bindings:
   ```eiffel
   map[B](f: A -> B): SEQ[B]
     variant
       Current
     do
       match Current
         case Empty then
           Empty
         case Leaf(v) then
           Leaf(f(v))
         case Concat(l, r, ln) then
           let ml = l.map(f)
           let mr = r.map(f)
           Concat(ml, mr, ln)
       end
     ensure
       same_length: Result.length = Current.length
       model_map: Result.model = LIST_OPS.map(Current.model, f)
     end
   ```

2. **`filter(keep: A -> BOOL): SEQ[A]` (Parallel Predicate Selection):**
   - Filters left and right subtrees in parallel and combines them with stored sum length `fl.length + fr.length`:
   ```eiffel
   filter(keep: A -> BOOL): SEQ[A]
     variant
       Current
     do
       match Current
         case Empty then
           Empty
         case Leaf(v) then
           if keep(v) then Leaf(v) else Empty end
         case Concat(l, r, _) then
           let fl = l.filter(keep)
           let fr = r.filter(keep)
           Concat(fl, fr, fl.length + fr.length)
       end
     ensure
       not_longer: Result.length <= Current.length
       model_filter: Result.model = LIST_OPS.filter(Current.model, keep)
     end
   ```

3. **`fold_left[B](f: (acc: B, x: A) -> B, init: B): B` (Left-to-Right Tree Reduction):**
   - Threads the accumulator through the left subtree first, then into the right subtree, matching `LIST_OPS.fold_left(Current.model, f, init)`:
   ```eiffel
   fold_left[B](f: (acc: B, x: A) -> B, init: B): B
     variant
       Current
     do
       match Current
         case Empty           then init
         case Leaf(v)         then f(init, v)
         case Concat(l, r, _) then r.fold_left(f, l.fold_left(f, init))
       end
     ensure
       model_fold_left: Result = LIST_OPS.fold_left(Current.model, f, init)
     end
   ```

4. **`fold_right[B](f: (x: A, acc: B) -> B, init: B): B` (Right-to-Left Tree Reduction):**
   - Threads the accumulator through the right subtree first, then into the left subtree, matching `LIST_OPS.fold_right(Current.model, f, init)`:
   ```eiffel
   fold_right[B](f: (x: A, acc: B) -> B, init: B): B
     variant
       Current
     do
       match Current
         case Empty           then init
         case Leaf(v)         then f(v, init)
         case Concat(l, r, _) then l.fold_right(f, r.fold_right(f, init))
       end
     ensure
       model_fold_right: Result = LIST_OPS.fold_right(Current.model, f, init)
     end
   ```

5. **Refinement & Algebraic Laws in `SEQ[A]` `specify {ANY}`:**
   - `law empty_is_empty`: `ensure empty_check: SEQ.empty.is_empty`
   - `law singleton_not_empty(x: A)`: `ensure non_empty: not SEQ.singleton(x).is_empty`
   - `law map_preserves_empty[B](f: A -> B)`: `ensure empty_mapped: SEQ.empty.map(f).model = LIST.empty`
   - `law map_singleton_refines[B](x: A, f: A -> B)`: `ensure single_mapped: SEQ.singleton(x).map(f).model = LIST.empty.prepend(f(x))`
   - `law fold_left_singleton[B](x: A, f: (acc: B, item: A) -> B, init: B)`: `ensure single_folded: SEQ.singleton(x).fold_left(f, init) = f(init, x)`
   - `law fold_right_singleton[B](x: A, f: (item: A, acc: B) -> B, init: B)`: `ensure single_folded: SEQ.singleton(x).fold_right(f, init) = f(x, init)`

---

### Task 3.3: Introduce `functions FUNCTION_OPS` (Identity & Pointwise Function Composition)

#### Addressing First-Order SMT Equality (Review §2.2 Friction Point 1)
As documented in [core_library_vs_functional_ideas_review.md](core_library_vs_functional_ideas_review.md#L41-L50), asserting structural equality `Result = (fn(x: A): C do f(g(x)) end)` on function closures is brittle in First-Order SMT because lambda-lifted closures are represented by environment records. Instead, `FUNCTION_OPS` specifies higher-order combinators **pointwise** over their domain.

#### Design & Specification for `FUNCTION_OPS`

1. **Group Header Update:**
   Update line 1 of `stdlib/stdlib.gdl` to export all 7 standard library units:
   ```eiffel
   group { SEQ, BIGNAT, STRING, OPTION, RESULT, LIST_OPS, FUNCTION_OPS }
   ```

2. **Combinators (`feature {ANY}`):**
   - `identity[A](x: A): A`:
     ```eiffel
     identity[A](x: A): A
       do
         x
       ensure
         is_id: Result = x
       end
     ```
   - `id_fn[A]: A -> A`:
     ```eiffel
     id_fn[A]: A -> A
       do
         fn(x: A): A do x end
       ensure
         pointwise_id: forall x: A. Result(x) = x
       end
     ```
   - `compose[A, B, C](f: B -> C, g: A -> B): A -> C`:
     ```eiffel
     compose[A, B, C](f: B -> C, g: A -> B): A -> C
       do
         fn(x: A): C do f(g(x)) end
       ensure
         pointwise: forall x: A. Result(x) = f(g(x))
       end
     ```

3. **Pointwise Algebraic Laws (`specify {ANY}`):**
   - `law identity_law[A](x: A)`:
     `ensure fixed: FUNCTION_OPS.identity(x) = x`
   - `law compose_pointwise[A, B, C](f: B -> C, g: A -> B, x: A)`:
     `ensure applied: FUNCTION_OPS.compose(f, g)(x) = f(g(x))`
   - `law compose_left_identity[A, B](f: A -> B, x: A)`:
     `ensure left_id: FUNCTION_OPS.compose(FUNCTION_OPS.id_fn, f)(x) = f(x)`
   - `law compose_right_identity[A, B](f: A -> B, x: A)`:
     `ensure right_id: FUNCTION_OPS.compose(f, FUNCTION_OPS.id_fn)(x) = f(x)`
   - `law compose_associative[A, B, C, D](f: C -> D, g: B -> C, h: A -> B, x: A)`:
     `ensure assoc: FUNCTION_OPS.compose(FUNCTION_OPS.compose(f, g), h)(x) = FUNCTION_OPS.compose(f, FUNCTION_OPS.compose(g, h))(x)`

---

## 3. Compiler (`gardelc`) Adjustments Required for Step 3

To verify `LIST_OPS`, the new `SEQ[A]` combinators, and `FUNCTION_OPS` with 0 errors in Boogie/Z3, Dafny, and the symbolic evaluator, we need five targeted enhancements in [gardelc/src/main.rs](../gardelc/src/main.rs):

1. **Multi-Argument First-Class Function Types (`(acc: B, x: A) -> B`) & `Fn2_app` in Boogie:**
   - **Boogie Prelude:** Add binary function type and application alongside `Fn1`:
     ```boogie
     type Fn2 A B C;
     function Fn2_app<A, B, C>(f: Fn2 A B C, x: A, y: B) returns (C);
     ```
   - **`map_boogie_type` ([gardelc/src/main.rs#L5068-L5072](../gardelc/src/main.rs#L5068-L5072)):**
     When `split_top_level_arrow(ty)` splits `(acc: B, x: A) -> B`, parse comma-separated parameter declarations inside `(...)`. If there are 2 parameters of types `T1` and `T2`, map to `(Fn2 <map(T1)> <map(T2)> <map(Ret)>)`.
   - **`emit_boogie_expr_with_strings` ([gardelc/src/main.rs#L6201-L6210](../gardelc/src/main.rs#L6201-L6210)):**
     When `Expr::Call(fname, args)` invokes a first-class function variable with `args.len() == 2`, emit `Fn2_app(fname, a0, a1)`.

2. **Chained Call / Postfix Invocation Syntax (`compose(f, g)(x)` and `Result(x)`):**
   - **Parser (`parse_postfix` at [gardelc/src/main.rs#L1700-L1723](../gardelc/src/main.rs#L1700-L1723)):**
     Currently `parse_postfix` handles `.member(...)` chains, while `Ident(...)` is only parsed in `parse_primary`. When an expression `e` (such as `FUNCTION_OPS.compose(f, g)` or `Result`) is followed by `(args)` on the same line, allow applying it as a function value (e.g., lowering `e(x)` to `Expr::MethodCall(Box::new(e), "apply".to_string(), args)` or `Fn1_app(<emit(e)>, <emit(x)>)` in Boogie).
   - **Lambda Combinator Lowering in Boogie (`Fn1_id` & `Fn1_compose`):**
     Declare in the Boogie prelude:
     ```boogie
     function Fn1_id<A>() returns (Fn1 A A);
     axiom (forall<A> x: A :: { Fn1_app(Fn1_id(), x) } Fn1_app(Fn1_id(), x) == x);
     function Fn1_compose<A, B, C>(f: Fn1 B C, g: Fn1 A B) returns (Fn1 A C);
     axiom (forall<A, B, C> f: Fn1 B C, g: Fn1 A B, x: A ::
       { Fn1_app(Fn1_compose(f, g), x) }
       Fn1_app(Fn1_compose(f, g), x) == Fn1_app(f, Fn1_app(g, x)));
     ```
     In `emit_boogie_expr_with_strings` for `Expr::Lambda(params, _, stmts, body)`, translate `fn(x: A): A do x end` to `(Fn1_id() : (Fn1 A A))` and `fn(x: A): C do f(g(x)) end` to `Fn1_compose(f, g)`. This gives Z3 exact first-order trigger patterns to discharge pointwise postconditions and associativity/identity laws instantaneously.

3. **Built-in Boogie `Seq` Theory Extensions for `LIST_OPS` (`Seq_map`, `Seq_filter`, `Seq_fold_left`, `Seq_fold_right`):**
   - In [gardelc/src/main.rs#L4300-L4322](../gardelc/src/main.rs#L4300-L4322), `LIST[A]` is modeled in Boogie as `Seq T` with `Seq_empty`, `Seq_prepend`, `Seq_append`, `Seq_head`, `Seq_tail`, `Seq_length`, and `Seq_has`.
   - Extend the `Seq` theory in the Boogie prelude with:
     - `function Seq_map<A, B>(xs: Seq A, f: Fn1 A B) returns (Seq B);`
     - `function Seq_filter<A>(xs: Seq A, keep: Fn1 A bool) returns (Seq A);`
     - `function Seq_fold_left<A, B>(xs: Seq A, f: Fn2 B A B, init: B) returns (B);`
     - `function Seq_fold_right<A, B>(xs: Seq A, f: Fn2 A B B, init: B) returns (B);`
   - Include their structural base/inductive axioms (`Seq_empty`, `Seq_prepend`) and append homomorphism axioms (`Seq_append`), and route `LIST_OPS.map(...)`, `LIST_OPS.filter(...)`, `LIST_OPS.fold_left(...)`, `LIST_OPS.fold_right(...)` in `emit_boogie_expr_with_strings` to `Seq_map`, `Seq_filter`, `Seq_fold_left`, `Seq_fold_right`.

4. **Cross-Generic Return Type & Invariant Handling on `SEQ[A].map[B]`:**
   - In `SEQ[A]`, `map[B](f: A -> B): SEQ[B]` returns `SEQ[B]` (an instantiation of `SEQ` with routine type parameter `B` rather than `A`).
   - Update `map_boogie_type` ([gardelc/src/main.rs#L5076-L5080](../gardelc/src/main.rs#L5076-L5080)) and `emit_boogie` so that inside `module SEQ[A]`, `SEQ[B]` maps to `(SEQ B)` rather than `(SEQ A)`, and constructor calls (`Empty`, `Leaf(f(v))`, `Concat(ml, mr, ln)`) in polymorphic routines ascribe their target return type `((Empty() : (SEQ B)))` just as we did for `RESULT[E, A]` in Step 2.

5. **Inductive Length-Bound Axiom on `SEQ[A]` (`length` vs `model.length`):**
   - Add the frame property `0 <= fn_length(_fuel, Current) && fn_length(_fuel, Current) == Seq_length(fn_model(_fuel, Current))` (or `in_u64` bound preservation on `filter`) so that inside `SEQ[A].filter`, Z3 knows `fl.length <= l.length` and `fr.length <= r.length`, proving `in_u64(fl.length + fr.length)` in the `Concat(l, r, ln)` branch without overflow.

---

## 4. Verification & Testing Strategy

1. **Module-by-Module Strict Verification:**
   - Run `gardelc stdlib/stdlib.gdl --strict` to verify all 7 modules (`SEQ`, `BIGNAT`, `STRING`, `OPTION`, `RESULT`, `LIST_OPS`, `FUNCTION_OPS`) via Boogie + Z3 with **0 errors and 0 warnings**.
2. **Refinement & Soundness Unit Tests:**
   - Add unit tests in `gardelc/src/main.rs` verifying:
     - Positive verification of `LIST_OPS`, `SEQ` combinators (`map`, `filter`, `fold_left`, `fold_right`), and `FUNCTION_OPS` (`identity`, `compose`, and pointwise laws).
     - Negative soundness test: a mutated `SEQ.map` or `SEQ.fold_left` implementation that swaps left and right subtrees (`Concat(mr, ml, ln)`) fails the `model_map` or `model_fold_left` refinement postcondition in Z3!
     - Parallelism test: verify that `SEQ.map` and `SEQ.filter` contain independent recursive `let` bindings (`ml`/`mr` and `fl`/`fr`) that `analyze_parallel_lets` groups into parallel fork-join tasks in Bend v2 and C11 (`#pragma omp task`).
3. **Single Source of Truth Synchronization:**
   - Synchronize `stdlib/stdlib.gdl` and `gardelc/src/main.rs:phase4_stdlib_source()`.
   - Run `cargo test --manifest-path gardelc/Cargo.toml` to ensure 100% pass rate across all existing and new Step 3 tests.

---

## 5. Execution Sequence & Milestones

| Milestone | Target | Description | Verification Criterion | Status |
| :--- | :--- | :--- | :--- | :---: |
| **M3.1** | `gardelc` HOF, `Fn2`, & `Seq` Theory | Add `Fn2_app`, `Fn1_id`/`Fn1_compose`, postfix call application `f(g)(x)`, `(SEQ B)` type mapping, and Boogie `Seq_map`/`filter`/`fold` homomorphism axioms. | Compiler builds cleanly; Boogie accepts multi-arg HOFs and pointwise lambda contracts with `/typeEncoding:p`. | ⏳ **Planned** |
| **M3.2** | `functions LIST_OPS` | Add `LIST_OPS` (`map`, `filter`, `fold_left`, `fold_right`) and 6 structural/append laws to `stdlib.gdl`. | `gardelc` verifies `LIST_OPS` and all 6 laws with 0 errors. | ⏳ **Planned** |
| **M3.3** | `module SEQ[A]` Combinators | Add `map[B]`, `filter`, `fold_left[B]`, `fold_right[B]` with `LIST_OPS` model refinement `ensure` clauses and 6 `specify` laws to `SEQ[A]`. | `gardelc` verifies `SEQ[A]` refinement proofs and no-overflow obligations with 0 errors; parallel `let` tasks emitted. | ⏳ **Planned** |
| **M3.4** | `functions FUNCTION_OPS` | Add `identity[A]`, `id_fn[A]`, `compose[A, B, C]`, and 5 pointwise algebraic laws (`compose_associative`, left/right identity) to `stdlib.gdl`. | `gardelc` verifies `FUNCTION_OPS` with 0 errors. | ⏳ **Planned** |
| **M3.5** | Sync & Regression Suite | Synchronize `stdlib/stdlib.gdl` with `phase4_stdlib_source()`, update `test_phase4_stdlib_package_verifies_all_modules`, and add Step 3 soundness/refinement tests. | `cargo test` passes 100%; `gardelc stdlib/stdlib.gdl --strict` passes with 0 errors, 0 warnings. | ⏳ **Planned** |

