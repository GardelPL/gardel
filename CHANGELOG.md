# Changelog

All notable changes, theoretical soundess fixes, verification overhauls, front-end language additions, backend code generation improvements, and standard library alignments for the Gardel programming language and `gardelc` compiler.

---

## [Unreleased]

Spec draft v0.5. [plan/Roadmap.md](plan/Roadmap.md) holds the detail for each task.

### Language and runtime
- **Concept bounds** (F.1). `a.less_than(b)`, `a.less_or_equal(b)` and `a.equals(b)` on a `K: ORD` translate, and mean the instance chosen at each use, in the proof and in C. A client may use a generic's contract as a fact about its type argument. `NAT`, `I64`, `U64`, `CHAR` and `BOOL` have these methods. `E0318` is a method that the bound does not give, and `E0320` a bound with no theory.
- **Functions in records and lists** (F.1). `r.handler(x)` applies a field of function type, in the proof and in C. A `LIST` of records holding functions builds, and has no `=` or `has`.
- **Fields of another module's record** (F.1) translate in the proof. `r.rank` from another module was `E0199`.
- **Contract compatibility check** (E.7). `gardelc --compare-with <old-dir>` reports what would break a client of an older version: a stronger `require`, a weaker `ensure`, removed or changed public items and concepts, a new `assume`, and a new or changed dependency. A change to a clause counts as compatible only if the solver proves it, and a new public name, a dependency that changed with no new version, or an `assume` the old version also had is a warning.
- **Qualified names** (E.5). `ALIAS::NAME` in expressions, types and type arguments, with `GARDEL` for the prelude, and `MODULE.Leaf(1)`, `ALIAS::MODULE.Leaf(1)` and `case MODULE.Leaf(x)` for constructors. A type known for certain breaks a tie between constructors. A user module may reuse any prelude name, so `E0101` is gone.
- **`let x: T = e` keeps `T`.** It was parsed and thrown away. It now settles a generic value, and a value known to have another type is `E0116`.
- **Packages** (E.4). `[dependencies]` in `Gardel.toml` adds a package by `path`. Two packages may use one module or constructor name, each keeping its own proofs, and a client sees only a dependency's contracts. `examples/hello_api` is an HTTP service built on the `ghttp` package.
- **Reactive programs** (Phase D): `reactive program`, with `init` and `step` as total routines, `STEP`, `COMMAND`, `EVENT`, `KEY` and `CONN_ID` in the prelude, timers, console input, TCP sockets, and event logs with replay.
- **Standard library** (Phase C): one module per file under `stdlib/`, verified UTF-8 `STRING` and `BYTES`, `RESULT`, and a law catalogue.
- **C backend**: every module lowers from source. Reference counting with ownership transfer, a thread-local allocator, and self tail calls as loops.
- **Dafny removed.** Boogie and Z3 are the only verifier, and a run has five steps. Solver limits count work, not time.

### Soundness fixes
- **A bound was not checked where a type was used** (F.1). `KV_MAP[STRING, NAT]` with `K: ORD` was accepted, and a generic body is proven only for what satisfies its bound. A written or inferred use is now `E0317`, and the C backend checks every instance again.
- **`x.code` was the identity on every receiver**, which is only true of `CHAR`. In a module with a routine `code` that returns its own type, a false `ensure` such as `x.code = x` verified. `code` is now the identity on a `CHAR` alone, and a module's own routine wins over every built-in name (`first`, `head`, `length`, `append`, `at`, `apply` and the rest), which gave `E0199` before. Bend had the same fault: a run that declares a `code` routine now calls it on every receiver.
- **Obligations reach every expression** (D.9). Overflow, division by zero, callee preconditions and `check` were skipped in lambdas, in `if` and `match` nested in an expression, in tuples, under `not`, in record fields and in a body `match`'s scrutinee. Such programs verified, then wrapped or crashed at run time.
- **`I64`** (D.10). `/` and `%` proved Euclidean results while the binary truncated, so `(-7) / 2 = -4` verified and the program computed `-3`. `-x` at `I64.min` and `I64.min / -1` had no check. Wrapping on `I64` was read as unsigned.
- **Private members** (E.0). Private routines and the constructors of a hidden representation were reachable from other modules, so a client could forge a value of an opaque type.
- **Laws and private routines across modules** (E.0, E.2). A law with a hidden `require` reached clients with no hypothesis, so `check absurd: False` verified. It is now a Rule 1 error. A private routine reached through a generic result, such as `o.get.secret`, passed `E0113` and was called with its `require` unchecked, so a verified program could crash. Name resolution now follows those receivers, and the verifier refuses a private target and any `require` it cannot state.

- **`assume` under `--strict`** (E.3). An `assume` nested in an expression, such as a `let` value or a call argument, was neither refused by `--strict` nor listed by `--audit`, so a false contract could verify at exit 0. Every expression and contract clause is now covered.

### Verifier
- **`.equals` on a match binder and on a quantified variable** (F.1) translate. `key.equals(k)` inside `case Field(key, ...)` in a function was `E0199`.
- **`variant` with no measure** (F.1) is `E0319`, not `E0199`.
- **Boogie 3.4.3 is pinned** in place of 3.1.6, which removes the shutdown hang (fixed in 3.2.4). 3.5.x is not used, as it fails `SEQ.filter` in about 1 of 7 runs. No verdict changed.
- **Continuous integration.** Every push to `main` or `libraries`, and every pull request, builds `gardelc`, verifies the stdlib and examples under `--strict` with no proof cache, and runs `cargo test`.
- **A hung `boogie /version` no longer turns the cache off** (F.0). Boogie 3.1.6 hangs for good in about 1 of 275 of these calls, as it shuts down, which made proof cache tests fail at random. Each call now gets 10 s and up to three tries, and only a hang is tried again. If a tool gives no version, a `[cache] off` line names it and the reason.
- **`--verify-deps` proves the prelude** that `gardelc` carries (E.4), as well as every module of the build.
- **Local verification cache** (E.3). A module this machine has proven is not given to the solver again, unless something that decides the answer changed: its verification condition, `gardelc`, Boogie, Z3, or the limits. The stdlib goes from 15.2 s to 0.4 s on a second run. The cache is outside the project, in `GARDEL_CACHE_DIR` or the user's cache directory, and an entry from anywhere else, or changed in any byte, is ignored. `--verify-deps` ignores the cache.
- **Verification conditions are reproducible.** One source could give different axiom order or lambda names from one run to the next.
- **Matching loops in the fusion axioms.** `LIST_OPS.filter(LIST.empty, p).length` and `LIST_OPS.map(l, identity)` could make Z3 run forever, past both of its limits. Fusion now stops at an empty list, and composing with the identity gives the same function.
- **`gardelc` enforces the solver's time backstop itself**, on the whole Boogie run and its child processes, and reports the module as unproven (`GARDEL_SOLVER_BACKSTOP`).

### Behaviour changes that can reject existing code
- Bitwise operators on `I64` are refused (`bitwise_on_signed`), and `-x` on `U64` needs `x = 0`.
- Unknown command-line flags are errors.
- An export list may not name a prelude module (`E0114`), and a declared name may not have the form `PK<n>_...` (`E0115`).
- A `let x: T = e` that contradicts `T` is refused (`E0116`), where `T` used to be ignored.
- A project constructor that clashes with a prelude one, such as a `Leaf`, is now a separate identity. A bare use from another module of the project means the project's, as before. Generated symbols for it are named `PK<n>_<PROJECT>_Leaf`, so its `.bpl`, `.c` and `.bend` text changes, though nothing else about them does.
- Two modules of a project with one constructor name each get a separate identity, so generated symbols change for those constructors as well.
- `--strict` now refuses a nested `assume`, and `--audit` lists it.
- A run that cannot start Boogie or Z3 is an error. It used to fall back to sampling and could report success on a false contract. `--bounded-check` still samples, on request.
- `--inspect` shows no interface when the run has errors, and exits with status 1.
- `--bounded-check` no longer reports its obligations as satisfied. Each module gets a `not_proven` warning, an error under `--strict`, and `--inspect` shows no interface for it.
- `gardelc DIR` on a directory with a `Gardel.toml` now reads the manifest and uses `src/`, as `--project DIR` does. It used to ignore the manifest and link every `.gdl` file below the directory. Combining such a directory with other inputs is an error.
- A law's proof no longer reaches its clients across a package boundary. It could link other modules into a client's verification.
- The package boundary now separates any two packages. It used to have one set of external modules, so two dependencies could prove facts from each other's bodies. Only tests can set it until path dependencies arrive.
- A bad or missing `--fuel` value is an error, where it used to run at the default fuel. A repeated `--fuel` takes the last value.
- `Gardel.toml` accepts only space and tab as whitespace. A no-break space or a form feed outside a string is an error, as in TOML.
- New rules `E0106` to `E0113` report undefined names and hidden members before verification. `E0199` reports what the verifier cannot accept.

---

## [0.4.0] - 2026-10-28

This release represents a comprehensive overhaul of the Gardel Language Specification (Draft v0.4) and the `gardelc` compiler implementation, closing critical theoretical soundness holes, replacing bounded concrete sampling with genuine Boogie/Z3 SMT verification, completing the front-end grammar and type system, implementing general AST-driven C11 and Bend v2 lowering, integrating Perceus reference counting, and completing the standard library.

The remediation and implementation proceeded across four distinct phases:

---

### Phase 1: Specification Soundness & Theoretical Semantics Patch
*Target: `spec/gardel_language_specification_draft_v0_5.md`*

- **Decoupled `EQ` from SMT Leibniz Identity (§9)**:
  - Replaced the contradictory law `agrees_with_structural` (`a.equals(b) = (a = b)`) in concept `EQ` with standard equivalence relation laws: `reflexive`, `symmetric`, and `transitive`.
  - Resolved the fatal soundness bug where non-canonical representation ADTs (such as balanced tree sequences `SEQ[A]`, un-normalized `BIGNAT`, and quotients) triggered contradictory equality axioms in Z3, allowing arbitrary false proofs.
  - Updated `ORD` laws (`less_or_equal`, `total`) to reference `a.equals(b)` rather than SMT structural equality `a = b`.
  - Introduced sub-concept `CANONICAL_EQ refines EQ` for flat/canonical types (`U64`, `BOOL`, `OPTION[U64]`) where structural identity implies behavioral equivalence.
- **Closed Invariant Destructuring & Routine Return Gap (§6, §12)**:
  - Unified routine verification obligations: both `create` routines and `feature` routines returning an instance of `Current` (or any refined type `M`) carry the postcondition obligation $I(\mathit{Result})$.
  - Formalized hereditary invariant requirements on inductive sum types (`case Node(l, r)`): invariants must be proven hereditary ($\forall l, r.\; I(\text{Node}(l, r)) \implies I(l) \land I(r)$) or scoped as ghost model properties at public boundaries. Clarified the use of local `check` blocks for inductive construction.
- **Formalized `UNIT`, `with IO` Monadic Typing, and Linearity (§3, §11, §15)**:
  - Added primitive type `UNIT` (literal `()`) to §3.
  - Defined explicit typing rules for monadic `with IO`:
    - `x <- e` requires $e : \text{IO}[A]$ and binds $x : A$.
    - Bare statement $e$ requires $e : \text{IO}[\text{UNIT}]$.
    - The final expression in `with IO ... end` must have type $\text{IO}[R]$, typing the entire block as $\text{IO}[R]$.
  - Formalized affine/linear consumption: values of type `IO[...]` must be consumed exactly once across every control-flow path, cannot be dropped in branch divergence, and cannot be captured in multi-use closures.
- **Corrected `F64` Confluence Claims & EBNF Grammar (§2, §3, §10, §15)**:
  - Scoped Church-Rosser bit-for-bit confluence claims strictly to exact discrete types (`U64`, `I64`, `MATH_INT`, `BOOL`, `CHAR`, and ADTs).
  - Specified `F64` as following strict IEEE-754 syntactic expression trees without compiler re-association or `-ffast-math` contractions (`-ffp-contract=off`).
  - Supported multi-parameter refined function types in EBNF: `"(" ( Param ( "," Param )* )? ")" "->" Type RequireBlock? EnsureBlock?`.
  - Specified newline-delimited (or optional `;`) statement termination in `Block` and `WithIoExpr` to eliminate `let` function application ambiguity.
  - Enforced lexical casing rules (`UpperIdent` for types, modules, concepts, and constructors; `LowerIdent` for variables, parameters, routines, fields, and tags).

---

### Phase 2: Verifier Soundness Overhaul & Real Boogie/Z3 Integration
*Target: `gardelc/src/main.rs`*

- **Replaced Bounded Concrete Sampling with Genuine Formal Verification**:
  - Eliminated the verification bypass where `verify_module_phase3` only evaluated concrete sample inputs `0, 1, 2` and `[]`, which previously allowed out-of-bounds errors, overflows, and false quantifiers (`forall x: NAT. x < 5`) to pass with 0 errors.
  - Integrated automated external solver discovery (`find_solvers`) resolving `boogie.exe` and `z3` from `BOOGIE_EXE`, `Z3_EXE`, local tool directories, user `.dotnet/tools`, and system `PATH`.
  - Wired direct Boogie and Z3 execution as the mandatory default verification engine. Introduced explicit `--bounded-check` flag strictly as an opt-in fallback.
  - Implemented structured SMT counterexample parsing and model translation: cleans SSA versions (`var@@1` -> `var`), reconstructs ADT records, extracts precondition path conditions, attaches suggested lemmas, and formats diagnostics into structured error reports.
  - Enforced that compilation halts with exit code 1 whenever formal verification fails.
- **Resolved Axiomatization Soundness Bugs in Boogie Emitter**:
  - **Variant Disjointness**: Emitted pairwise exclusion axioms (`is_V1(x) ==> !is_V2(x)`) and bidirectional equivalence axioms for nullary variants (`is_Empty(x) <==> x == Empty()`), preventing false branch satisfaction in Z3.
  - **String Interning**: Replaced single hardcoded `IO_read_line()` emission with interned unique constants (`const unique str_lit_{n}: GardelString`) and string length axioms, preventing distinct string literals from falsely evaluating as equal.
  - **Canonical Record Field Ordering**: Sorted record fields in `emit_boogie_expr`, `emit_dafny_expr`, and symbolic evaluators to strictly match product representation declaration order.
  - **General Recursive Variant Decrease Checks**: Implemented non-negativity and strictly decreasing checks for general numeric and measure variants across recursive `Call` and `MethodCall` expressions.
  - **Capture-Avoiding Substitution**: Implemented safe substitution in Boogie definitional axioms that respects variable shadowing in `let`, `lambda`, `match`, and `with IO`.

---

### Phase 3: Front-End Completeness, Concept System & Multi-Module Pipeline
*Target: `gardelc/src/main.rs`, `stdlib/stdlib.gdl`*

- **Full Front-End Lexer & Parser Completeness (§3, §4, §5, §8, §15)**:
  - Added support for unit literal `()`, floating-point literals (`1.23`), character literals (`'a'`), unary negation (`-x`), wrapping arithmetic (`+%`, `-%`, `*%`), and bitwise operators (`&`, `|`, `^`, `<<`, `>>`).
  - Mapped signed `I64` to bounded integers with signed overflow checks; mapped wrapping and bitwise operations on `U64` to Boogie `bv64` bitvector functions with shift-bounds checks (`0 <= rhs < 64`).
  - Made `implies` and function arrow `->` right-associative in the parser.
  - Enforced lexical casing (`UpperIdent` vs. `LowerIdent`) and line-break/`;` statement delimitation.
  - Added module kind restrictions rejecting `representation`, `create`, or `invariant` in `functions` and `program` declarations.
  - Implemented recursive nested pattern matching AST (`Pattern`) and comma-separated lexicographic termination variants.
- **Type Checking, Scope-Aware Visibility & Linear IO (§4, §6, §11)**:
  - Implemented `collect_symbols_scoped` tracking in-scope bindings to prevent false visibility errors when local variables shadow private field names.
  - Added hereditary invariant verification procedures (`Verify_Hereditary_Invariant_*`) asserting invariants hold inductively on recursive ADT destructuring.
  - Implemented monadic IO return checking and branch linearity verification (`check_branch_io_linearity`) enforcing balanced linear IO sequencing across `if`/`else` branches.
- **Concept Signatures, Constrained Generics, Monomorphization & Closure Conversion (§8, §9, §13)**:
  - Implemented full signature verification in `check_and_expand_concepts` validating parameter counts, parameter types (with `Current` mapped), return types, and `where` constraints.
  - Emitted uninterpreted Boogie functions and mathematical axioms for constrained generics (`[A: ORD]`, `[A: EQ]`).
  - Implemented full AST monomorphization across product/sum representations and routine signatures, with automatic instance discovery in `ProgramEnv`.
  - Implemented closure conversion (`closure_convert_module`) lowering capturing lambdas into environment records (`ClosureEnv_<id>`) and top-level combinators (`closure_apply_<id>`).
- **Multi-Module Compilation Pipeline**:
  - Upgraded code generation pipeline to iterate over all modules in a compilation unit, generating `.bpl`, `.dfy`, `.bend`, and `.c` for every module in `build/`.
  - Stamped `Diagnostic` items with the originating `module_name` for accurate multi-module JSON reporting.

---

### Phase 4: General AST Lowering, Perceus RC, Automatic Parallelism & Stdlib
*Target: `gardelc/src/main.rs`, `stdlib/stdlib.gdl`, `examples/`*

- **General AST-Driven C11 & Bend v2 Code Generation (§5, §8, §13)**:
  - Eliminated hardcoded demo C11 string templates in favor of general AST-driven lowering for product structs, tagged union sum types, pattern matching, constructor functions, and expressions.
  - Integrated `#pragma STDC FP_CONTRACT OFF` in `emit_c11_runtime_header()` to enforce strict IEEE-754 floating-point evaluation without platform-dependent contraction.
  - Lowered closure environments and top-level combinators into native C structs and function pointers.
  - Lowered `Expr::WithIo` bindings and statements into sequential C execution, emitting `main(void)` and linking clean executables with host C compilers (gcc/clang).
- **Automatic Fork-Join Parallelism & Tail-Call Optimization (§10)**:
  - Implemented static dependency analysis (`analyze_parallel_lets`) partitioning independent recursive `let` bindings into parallel groups.
  - Emitted parallel tuple assignments `(ml, mr) = (e1, e2)` for Bend v2 and `#pragma omp task shared(...)` / `#pragma omp taskwait` for C11.
  - Added configurable parallelism granularity cutoff `#ifndef GARDEL_PAR_THRESHOLD #define GARDEL_PAR_THRESHOLD 512` to prevent task explosion on small inputs.
  - Implemented self-recursive tail call detection (`is_tail_recursive_routine`) lowering recursive functions to iterative loops (`tail_loop:`) in C11.
- **Perceus Reference Counting & Memory Management (§13, §16)**:
  - Added atomic reference count fields (`_rc`) to heap-allocated ADT structs with constructor initializers (`_rc = 1`).
  - Added allocation tracking runtime primitives (`gardel_alloc`, `gardel_free`, `gardel_live_allocations`) to ensure leak-free execution and verify reference lifecycle correctness.
- **Standard Library Consolidation & Examples Decoupling**:
  - Upgraded the canonical standard library [stdlib/stdlib.gdl](stdlib/stdlib.gdl) to contain pure data types and collections: `group { SEQ, BIGNAT, STRING, OPTION }`:
    - `SEQ[A]`: Functional balanced tree sequences with logarithmic indexing, concatenation, slices, and verified invariants.
    - `BIGNAT`: Arbitrary-precision unsigned integers backed by `SEQ[U64]` with ghost `MATH_INT` polynomial models.
    - `STRING`: UTF-8 string slices backed by `SEQ[U64]` with ghost `LIST[CHAR]` models.
    - `OPTION[A]`: Verified optional values with `is_some`, `is_none`, and safe extraction.
  - Decoupled `program GREETER` from the canonical library and relocated it to [examples/greeter.gdl](examples/greeter.gdl) as a standalone demonstration of interactive monadic IO programming.

---

### Verification and Test Suite Growth

The unit test suite in `gardelc/src/main.rs` was expanded from initial stubs to 27 comprehensive, fully automated tests with a 100% pass rate:

1. `test_single_module_pipeline_v0_4`
2. `test_boogie_fuel_monomorphization_bounded_stack`
3. `test_phase3_boogie_emission_and_fuel`
4. `test_phase3_overflow_counterexample_translation`
5. `test_phase3_fuel_exhaustion_and_reveal_fix`
6. `test_phase3_assume_audit_and_strict`
7. `test_phase4_stdlib_package_verifies_all_modules`
8. `test_phase4_io_boundary_and_linearity_firewall`
9. `test_phase4_inspect_module_interface`
10. `test_phase2_variant_disjointness_and_distinct_strings`
11. `test_phase2_canonical_record_ordering`
12. `test_phase2_capture_avoiding_substitution`
13. `test_phase2_general_numeric_variant_termination`
14. `test_plan3_front_end_primitives_and_operators`
15. `test_plan3_right_associative_implies_and_casing`
16. `test_plan3_module_kind_restrictions`
17. `test_plan3_nested_pattern_matching_and_lexicographic_variants`
18. `test_plan3_hereditary_invariant_verification`
19. `test_plan3_io_branch_linearity_checking`
20. `test_plan3_concept_signatures_and_ord_axiomatization`
21. `test_plan3_multi_module_stdlib_pipeline`
22. `test_plan3_closure_conversion_and_refined_fn_types`
23. `test_plan4_general_c11_ast_lowering_custom_adt`
24. `test_plan4_c11_and_bend_closure_conversion_codegen`
25. `test_plan4_automatic_fork_join_parallel_let_analysis`
26. `test_plan4_tail_recursion_to_c11_while_loop`
27. `test_plan4_perceus_refcounting_zero_memory_leaks`

