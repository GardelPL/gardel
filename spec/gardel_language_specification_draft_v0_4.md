# Gardel Language Specification (Draft v0.4)

Oct 27, 2026 · @Javier

## 1. Overview and changes since v0.2 / v0.3

Gardel is a pure, total, deterministic, statically verified language designed for AI-driven code generation and human readability. Version 0.4 transitions the execution backend from HVM2 (24-bit interaction nets) to **Bend v2 / BendRT** (portable C11 for single-core and multi-core CPUs, plus CUDA and Apple Metal for GPUs), upgrades numerics to standard 64-bit machine words, and removes special-purpose recursion constructs (`fold` and `unfold`) in favor of uniform structural recursion with `let` bindings.

| Area | Problem in v0.2 / v0.3 | Resolution in v0.4 |
| --- | --- | --- |
| Backend target | HVM2 restricted numbers to 24 bits, ran 3x–5x slower than C on single cores, and locked GPUs to NVIDIA CUDA | Target **Bend v2 / BendRT** (and portable C11), unlocking native C single-core speed and parallel CPU, CUDA, and Metal execution (§10, §13) |
| Numerics | `U24`/`I24`/`F24` overflowed at 16.7 million due to HVM2 node tags; missing bitwise operators | Standard 64-bit `U64` (`NAT`), `I64` (`INT`), and `F64` with Z3 overflow proofs, bitwise operators, and deterministic IEEE-754 evaluation (§3) |
| Recursion & Parallelism | `fold` and `unfold` caused variable-type shadowing, duplicated `ensure` blocks, and lacked postconditions on `unfold` | Removed `fold`, `unfold`, `fork`, `Subject`, and `@`. All recursion uses standard `match`/`if` and `variant`; independent `let` bindings run in parallel (§10, §14) |
| Local bindings | Grammar had no `let` binding, forcing repeated expressions and preventing intermediate `check` references | Added `Block` (`Stmt* Expr`) supporting `let x = e`, `check`, `assume`, and `reveal` inside `do`, `then`, `else`, and `case` branches (§5, §15) |
| Closures | Closures capturing closures were marked affine due to HVM2 lambda-duplication anomalies | Compile-time closure conversion (lambda lifting to first-order ADT records) allows unrestricted closure use and parallel duplication (§8, §13) |
| Invariant visibility | A friend module in `representation {FRIEND}` could fail to verify if the invariant called private queries | Added Visibility Rule 5: every symbol in an `invariant` must be visible to at least the representation's visibility list (§4) |
| AI Diagnostics | Solver failures only described human-oriented error text | Specified `gardelc --json-diagnostics` with structured counterexamples and contract tags for automated LLM repair loops (§12, §13) |

---

## 2. Core design principles

Gardel trades mutation, inheritance, and ad-hoc loops for mathematical proofs, strict determinism, and implicit parallelism:

1. **One module, one ADT.** A `module` defines one data type, its operations, and its invariant. `functions` and `program` modules hold free functions and entry points.
2. **Pure, total, deterministic.** Every routine terminates and nothing mutates. Evaluation order never changes a result for exact discrete types (`U64`, `I64`, `MATH_INT`, `BOOL`, `CHAR`, and algebraic data types, satisfying strict Church-Rosser bit-for-bit confluence). `F64` operations follow strict IEEE-754 expression-tree semantics without compiler re-association. Command-query separation becomes queries versus transformers that return a new value. `old` and `modifies` are unnecessary.
3. **Contracts are proofs, not checks.** `require`, `ensure`, `invariant`, `variant`, and `check` are discharged by the verifier and erased. Nothing is evaluated at runtime.
4. **Invariants are types.** A module's invariant is part of what its type means, so it holds for every value wherever that value travels.
5. **Selective export is a verification firewall.** Visibility lists control both name access and which facts the solver sees.
6. **Concepts with laws replace inheritance.** Polymorphism comes from `concept` declarations whose laws are proven, guaranteeing unique, deterministic behavior across generic code.
7. **Runtime numbers are 64-bit machine numbers.** Contracts reason in unbounded mathematics (`MATH_INT`), but every runtime operation must be proven not to overflow.
8. **Erase to Bend v2 and C.** After verification, only pure ADTs, tail/divide-and-conquer recursion, and native 64-bit arithmetic remain, compiling to Bend v2 / BendRT for parallel CPU and GPU execution.

---

## 3. Primitive types and numerics

Runtime numbers are standard 64-bit machine values, and every checked arithmetic operation carries an overflow proof obligation. This removes the gap between what Z3 proves and what the CPU or GPU computes while avoiding HVM2's 24-bit ceiling.

| Type | Runtime representation | Meaning in contracts |
| --- | --- | --- |
| `UNIT` (alias `()`) | Bend v2 `0` / C `void` (0-byte singleton) | unit value `()`; empty tuple / zero-argument return |
| `U64` (alias `NAT`) | Bend v2 `U64` / C `uint64_t` | integer in $[0, 2^{64} - 1]$; `NAT.max` $= 18\,446\,744\,073\,709\,551\,615$ |
| `I64` (alias `INT`) | Bend v2 `I64` / C `int64_t` | integer in $[-2^{63}, 2^{63} - 1]$ |
| `F64` | Bend v2 `F64` / C `double` | IEEE-754 binary64 values with bound axioms; strict expression-tree evaluation |
| `BOOL`, `CHAR` | `uint8_t`, `uint32_t` | booleans; Unicode scalar values |
| `MATH_INT` | none (ghost) | unbounded integer; legal only in `specify` and contracts |

Rules:

- **Contracts use mathematics.** Runtime numbers lift implicitly to `MATH_INT` inside contracts, so `Result = a + b` in an `ensure` means the true mathematical sum.
- **Checked operators.** `+`, `-`, `*` on runtime numbers generate a proof obligation that the mathematical result is in $[0, 2^{64} - 1]$ for `U64` (or $[-2^{63}, 2^{63} - 1]$ for `I64`). `/` and `%` require a non-zero divisor (and for `I64`, `a != INT.min or b != -1`).
- **Wrapping and bitwise operators.** `+%`, `-%`, `*%` are total and defined modulo $2^{64}$. Bitwise operators `&`, `|`, `^`, `<<`, `>>` operate on `U64` (with shift amounts required to be $< 64$). They are verified with Z3's `(_ BitVec 64)` theory and never mix with checked operators without an explicit conversion.
- **Deterministic floating point.** `F64` operations follow strict IEEE-754 evaluation order matching the syntactic expression tree. Compiler re-association (such as `-ffast-math` contractions or fused multiply-add `fma` synthesis across distinct operations) is strictly forbidden (`-ffp-contract=off`), and quiet/signaling NaN representations are canonicalized, ensuring that execution along a given expression tree is deterministic across CPU and GPU backends. However, because floating-point addition and multiplication are mathematically non-associative ($(a+b)+c \neq a+(b+c)$), parallel reductions do not claim bit-for-bit equivalence across dynamically re-partitioned trees; exact Church-Rosser confluence is guaranteed for discrete types (`NAT`, `INT`, `MATH_INT`, `BOOL`, `CHAR`, and ADTs).
- **Unit type.** `UNIT` is a primitive type with exactly one value, written `()`. It represents computations evaluated purely for sequenced effects (such as `IO.print(...) : IO[UNIT]`), and erases at runtime.
- **Conversions are explicit.** `NAT.of(m)` from a `MATH_INT` requires a proof that `0 <= m and m <= NAT.max`.
- **Large numbers.** A library type `BIGNAT` stores limbs in a `SEQ[U64]` and has `MATH_INT` as its ghost model.

The classic midpoint shows the effect. The version below verifies; `(lo + hi) / 2` is rejected because `lo + hi` may exceed `NAT.max`.

```eiffel
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

---

## 4. Module architecture

A `module` is one abstract data type organized into four visibility-controlled sections. Two further kinds cover what a pure ADT cannot: a `functions` module holds free functions and a `program` holds the entry point.

Inside a module, `Current` is the immutable receiver, `Result` is the return value in `ensure` clauses, and field projection and zero-argument queries share the same dot syntax (uniform access).

| Section | Purpose | Compiled? | Visible to the solver in client `M` |
| --- | --- | --- | --- |
| `representation {Vis}` | Sum or product layout of the type | Yes | Only if `M` is in `{Vis}`; otherwise the type is opaque. `{NONE}` hides it from all clients |
| `create {Vis}` | Constructors that need no receiver | Yes | Signature and `ensure` if `M` is in `{Vis}` |
| `feature {Vis}` | Runtime queries and transformers on `Current` | Yes | Signature and contracts if `M` is in `{Vis}` |
| `specify {Vis}` | Ghost queries, lemmas, concept law proofs | No (erased) | Usable in contracts of `M` if `M` is in `{Vis}`; forbidden in `do` bodies |

Sections may appear in any order, and `feature` and `specify` may repeat with different visibility lists. A module-wide `invariant` closes the module (§6).

**Visibility rules** (enforced during name resolution):

1. **Contract visibility.** Every symbol in a `require` clause of a routine exported to `{Vis}` must itself be exported to at least `{Vis}`. Callers must be able to read what they are asked to prove.
2. **Visible ensures only.** A client sees only the `ensure` clauses whose symbols it may see. A hidden clause stays usable inside the module, and the compiler warns when a client-facing routine loses clauses this way.
3. **Ghost stays ghost.** `specify` symbols may appear in any contract (`require`, `ensure`, `invariant`, `check`, `assume`, `variant`) but never in runtime expressions.
4. **Runtime-checkable preconditions.** A precondition of an exported routine should be written with exported runtime queries so callers can branch on it. Otherwise the compiler warns `ghost_precondition` (§7).
5. **Invariant visibility.** Every symbol referenced in a module's `invariant` must be exported to at least the visibility list of its `representation` section, ensuring friend modules that construct values directly can see and prove the invariant (§6).

A `functions` module has `feature` and `specify` sections only. A `program` has a `main` routine and is the only module that may use `IO` (§11).

---

## 5. Data representations and local bindings

A `representation` block declares either a product (named fields) or a sum (variants), and its visibility list decides who may build and take apart values directly.

- **Product.** Fields are declared as `name: Type`. A record literal is `{ f = v, g = w }` and a functional update is `{ x with f = v }`.
- **Sum.** Variants are declared as `| Name(field: Type, ...)`. Clients in the visibility list may construct variants and use them in `match`.
- **Local `let` bindings and statement blocks.** The body of any `do`, `then`, `else`, or `case` branch is a `Block` consisting of zero or more statements (`let name = expr`, `check`, `assume`, `reveal`) followed by a final result expression. `let` bindings are immutable and enter the proof context (`name = expr`) for all subsequent statements and the final expression.
- **Refinement-aware matching.** Exhaustiveness is checked against the proof context. A branch that the context rules out (for example `None` under `require Current.is_some`) may be omitted. A reachable branch that is missing is an error. Omitted branches generate no code, and the verifier has proven they cannot run.
- **Opaque and friend representations.** `representation {NONE}` hides the layout from every client. `representation {M1, M2}` shares it with named friends. Any direct construction outside the module triggers the invariant check of §6.
- **Mutual recursion.** Types that refer to each other are declared in one `group { A, B }` file. The verifier treats the group as a single unit for termination and invariants.

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

  get: A
    require
      present: Current.is_some
    do
      match Current
        case Some(v) then v
      end
    end

end
```

---

## 6. Invariants as refinement types

A module's `invariant` is part of its type: every value of type `M` anywhere in a program satisfies it. Formally, `M` denotes the refinement type $\{ x : M_{\text{raw}} \mid I(x) \}$, where $I$ is the conjunction of the invariant clauses.

- **Checked at every construction site.** A record literal, functional update, or variant constructor that produces an `M` is a construction site. The verifier must prove $I(v)$ there, using the path conditions, local `let` definitions, the `require` clauses, and $I(\mathit{Current})$. It does not matter whether the value is returned directly or nested in a `TUPLE`, `OPTION`, or `LIST`. Local `check` blocks or lemmas may immediately precede construction sites to establish inductive properties required by $I(v)$.
- **Assumed everywhere else.** Any value of type `M` can be used knowing $I(x)$ with no re-proof, because values never change.
- **Destructuring and recursive sum types.** When destructuring an inductive sum type (e.g., `match t` with `case Node(l, r)`), the branch context assumes subterms satisfy the module invariant ($I(l)$ and $I(r)$) if and only if $I$ is proven hereditary (i.e., inductive over subterms: $\forall l, r.\; I(\text{Node}(l, r)) \implies I(l) \land I(r)$). For non-hereditary predicates (such as global tree balance or key uniqueness), invariants on recursive ADTs must either provide an inductive heredity proof or be formulated as ghost model properties verified at public module boundaries.
- **Privileged modules.** A friend module listed in `representation {FRIEND}` that builds an `M` directly pays the same obligation at its own construction site (guaranteed visible by Visibility Rule 5 in §4).
- **Generic containers need no special rules.** `LIST[M]` holds only values that already satisfy $I$.

The bounded stack below stores its count so that every operation is constant time and the `U64` overflow checks of §3 are proven automatically from the invariant.

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

The record literal assigned to `rest` inside `pop` is a construction site, so the verifier proves `size - 1 = items.tail.length` and `size - 1 <= limit` there. The addition `Current.size + 1` in `push` cannot overflow `NAT.max` because `size < limit <= NAT.max` follows from `not_full` and the invariant.

---

## 7. Contracts, ghost code and the proof boundary

Contracts are never evaluated at runtime, so a program must be able to establish every precondition from data it actually has. Gardel provides five mechanisms for this and explicit rules for when the solver needs guidance.

1. **Ghost versus runtime.** `specify` queries exist only for proofs. `feature` queries are compiled. Contracts may call both, and `do` bodies (outside `check`/`assume`) may call only runtime queries.
2. **Path conditions.** The condition of an `if` and the pattern of a `match` enter the proof context of each branch, so a runtime test discharges a precondition:

   ```eiffel
   push_or_same(x: A): BOUNDED_STACK[A]
     do
       if Current.count < Current.capacity
         then Current.push(x)
         else Current
       end
     end
   ```
3. **Runtime-checkable preconditions.** Write preconditions of exported routines with exported runtime queries, as `push` does with `count` and `capacity`. A precondition that uses only ghost symbols draws the warning `ghost_precondition`, because callers can then only discharge it statically.
4. **Checked constructors at the boundary.** Untrusted data from outside becomes an invariant-carrying value only through a routine that returns `OPTION` or `RESULT`:

   ```eiffel
   try_make(cap: NAT): OPTION[BOUNDED_STACK[A]]
     do
       if cap > 0
         then Some(BOUNDED_STACK.make(cap))
         else None
       end
     ensure
       accepted: Result.is_some = (cap > 0)
     end

   try_make_result(cap: NAT): RESULT[STRING, BOUNDED_STACK[A]]
     do
       if cap > 0
         then Ok(BOUNDED_STACK.make(cap))
         else Error("capacity must be positive")
       end
     ensure
       accepted: Result.is_ok = (cap > 0)
       err_message: Result.is_error implies Result.get_error = "capacity must be positive"
     end
   ```
5. **Opaque bodies and fuel.** Outside its module a function is known only by its signature and `ensure` clauses. Inside, recursive functions unfold up to a `fuel` limit (default 2), and `reveal f` unfolds one definition fully for the remainder of the current block. State key facts as `ensure` clauses so clients can use them without unfolding.

**When the solver returns unknown.** The build fails. The author or AI agent may add `check tag: pred end` hints, bind intermediate values with `let`, split the proof into lemmas under `specify`, or write `assume tag: predicate end`. An `assume` adds the fact without proof. `gardelc --audit` lists every `assume`, and `--strict` rejects them.

---

## 8. Functions, closures and higher-order code

Functions are first-class values that carry contracts in their types, so libraries such as `map` and `filter` can be specified and verified.

- **Function types.** `A -> B` is a total, pure function. A refined function type names its parameter and adds clauses: `(x: NAT) -> NAT require small: x < 100 ensure bounded: Result <= x`.
- **Lambdas.** `fn(x: NAT): NAT do x + 1 end` is checked against the contract of the expected function type at the point of use.
- **Totality.** Function arguments must be total. There are no partial functions and no exceptions.
- **Compile-time closure conversion (lambda lifting).** Any lambda that captures variables from its enclosing scope is transformed by `gardelc` into a first-order environment ADT record paired with a top-level combinator function before Bend v2 / C emission. Because environments are standard first-order values ( satisfying their captured types' invariants), closures may be freely duplicated across parallel branches without affine restrictions or interaction-net superposition anomalies.

```eiffel
functions LIST_OPS

feature {ANY}

  map[A, B](xs: LIST[A], f: A -> B): LIST[B]
    variant
      xs
    do
      match xs
        case Empty      then Empty
        case Cons(h, t) then Cons(f(h), map(t, f))
      end
    ensure
      same_length: Result.length = xs.length
    end

  filter[A](xs: LIST[A], keep: A -> BOOL): LIST[A]
    variant
      xs
    do
      match xs
        case Empty      then Empty
        case Cons(h, t) then
          let rest = filter(t, keep)
          if keep(h) then Cons(h, rest) else rest end
      end
    ensure
      not_longer: Result.length <= xs.length
    end

end
```

---

## 9. Concepts, laws and conformance

A `concept` lists required operations and the laws they must obey, and a module conforms with `satisfies` and proves the laws in a `specify` block. Gardel has no implementation inheritance and no subtyping.

`=` in contracts denotes built-in structural Leibniz equality in SMT. The concept `EQ` defines behavioral equivalence via standard equivalence relation laws (reflexive, symmetric, transitive). This decouples behavioral equivalence from structural term identity, allowing collection and quotient types with non-canonical representations (such as `SEQ[A]`, `SET[A]`, and `RAT`) to conform soundly without introducing SMT contradictions. For types where structural representation is canonical and bijective with mathematical identity (such as `U64`, `BOOL`, or `OPTION[U64]`), the sub-concept `CANONICAL_EQ` refines `EQ` and requires agreement with structural equality. `ORD` refines `EQ` and requires a total order consistent with `a.equals(b)`.

```eiffel
concept EQ

  equals(other: Current): BOOL

  law reflexive(a: Current)
    ensure
      refl: a.equals(a)
    end

  law symmetric(a: Current, b: Current)
    require
      ab: a.equals(b)
    ensure
      ba: b.equals(a)
    end

  law transitive(a: Current, b: Current, c: Current)
    require
      ab: a.equals(b)
      bc: b.equals(c)
    ensure
      ac: a.equals(c)
    end

end

concept CANONICAL_EQ refines EQ

  law agrees_with_structural(a: Current, b: Current)
    ensure
      sound: a.equals(b) = (a = b)
    end

end

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

Rules:

- **Proofs.** Each law is proven in a `specify {CONCEPT}` block, by structural recursion with a `variant` when the type is recursive.
- **Conditional conformance.** `module LIST[A] satisfies EQ where A satisfies EQ` makes a generic module conform only when its parameter does.
- **Constrained generics.** In `module SORTED_LIST[A: ORD]` the laws of `ORD` enter the proof context as quantified axioms about `A`.
- **Triggers.** Each law is instantiated by an SMT trigger pattern. The default is the predicates of its `require` clause, and `trigger a.less_than(b)` overrides it to prevent quantifier instantiation loops.
- **No runtime dictionaries.** All generic code is monomorphized before Bend v2 / C codegen.

---

## 10. Parallelism: divide-and-conquer and BendRT

Gardel removes the special-purpose `fold` and `unfold` syntax of v0.2. Instead, parallelism arises directly from standard divide-and-conquer recursion, `let` bindings, and the `SEQ[A]` collection type, compiled to **Bend v2 / BendRT**.

1. **Deterministic confluence.** Because every routine is pure and total (§2), evaluating two sub-expressions in parallel or sequentially always produces the exact same value for all exact discrete types (`U64`, `I64`, `MATH_INT`, `BOOL`, `CHAR`, and ADTs). Race conditions and schedule-dependent outputs cannot exist. Floating-point (`F64`) expressions strictly preserve their specified expression-tree structure without compiler re-association.
2. **Automatic fork-join on independent bindings.** When a block contains two or more independent `let` bindings (or constructor arguments) that make recursive calls—such as `let ml = l.max_value` followed by `let mr = r.max_value` where `mr` does not depend on `ml`—`gardelc` emits a parallel fork (`ml mr = max_value(l) max_value(r)` in Bend v2 / BendRT task rings).
3. **Tail recursion to loops.** Self-recursive tail calls are lowered directly to native C loops / BendRT tail jumps with zero stack frame allocation.
4. **Tree generation without `unfold`.** Building a balanced parallel structure from a scalar seed uses a standard recursive routine with a decreasing numeric `variant` (such as `variant hi - lo`), giving the generator full access to `require` and `ensure` contracts (§14).
5. **SEQ.** The default collection for parallel work is `SEQ[A]`, a balanced binary tree with stored `U64` length and a ghost `model: LIST[A]`. Standard recursive operations over `SEQ` split work evenly across CPU threads or GPU SIMD groups. `LIST[A]` remains for specifications and small sequential data.

---

## 11. Effects and IO

Gardel keeps a pure core and moves all input and output to the program boundary through a linear action type `IO[A]`.

1. **Where IO lives.** Only a `program` module and routines typed `IO[A]` may perform effects. Core modules stay pure and verified.
2. **Linear actions and affine consumption.** An `IO[A]` value represents an unexecuted linear action token that must be consumed exactly once along every execution path in a `with IO` sequence. An `IO[...]` value cannot be duplicated, dropped silently in a branch, or captured by a multi-use closure (`fn`). Every branch of a conditional (`if` or `match`) within an IO routine must uniformly consume or return an `IO` action.
3. **Typing rules for `with IO`.** Sequences within a `with IO` block obey explicit monadic sequencing rules:
   - Binding statement `x <- e`: requires $e : \text{IO}[A]$, and binds $x : A$ in all subsequent statements of the block.
   - Bare statement $e$: requires $e : \text{IO}[\text{UNIT}]$, executing the effect and discarding the unit result.
   - Final expression: the trailing expression $e_{\text{final}}$ of `with IO ... end` must have type $\text{IO}[R]$, making the entire `with IO` block evaluate to $\text{IO}[R]$.
4. **External data is untrusted.** Bytes, strings, and numbers enter as raw values. They become invariant-carrying ADTs only through `create` or boundary routines that return `OPTION` or `RESULT` (§7).
5. **Limited specifications.** Contracts on an `IO` routine cover its pure arguments and result. The verifier does not model the outside world.

```eiffel
program GREETER

feature {ANY}

  main(args: LIST[STRING]): IO[EXIT_CODE]
    do
      with IO
        name <- IO.read_line
        IO.print("Hello, " + name)
        IO.exit(0)
      end
    end

end
```

---

## 12. Verification semantics and AI diagnostics

Every proof obligation reduces to a validity query for Boogie and Z3. Contracts use unbounded integers (`MATH_INT`), and 64-bit bitvectors appear only where wrapping or bitwise operators are used.

Let $I$ be the invariant of a module, $P$ the `require` clauses, and $Q$ the `ensure` clauses. The two core routine obligations are:

$$\forall \bar{a}.\; P(\bar{a}) \Rightarrow Q(\bar{a}, \mathit{Result}) \land I(\mathit{Result}) \qquad \text{(create)}$$

$$\forall c, \bar{a}.\; I(c) \land P(c, \bar{a}) \Rightarrow Q(c, \bar{a}, \mathit{Result}) \land ( \mathit{Result} : M \implies I(\mathit{Result}) ) \qquad \text{(routine on Current)}$$

Both `create` and `feature` routines returning an instance of `Current` (or any refined type $M$) carry the explicit postcondition obligation $I(\mathit{Result})$. This ensures that whenever a routine produces or transforms a value of type $M$—including via conditional branches, function compositions, or generic wrappers—the invariant is verified at the routine interface boundary in addition to checking construction sites.

| Obligation | Generated at | Must be proven |
| --- | --- | --- |
| Creation | each `create` routine | $P$ implies $Q$ and $I(\mathit{Result})$ |
| Routine | each `feature` and `specify` routine | $I(\mathit{Current})$ and $P$ imply $Q$, plus $I(\mathit{Result})$ if returning `Current` or a refined type $M$ |
| Construction site | every record literal, update, or variant constructor that builds an `M` | active context implies $I(v)$ |
| Overflow & Shift | each checked `+`, `-`, `*`, `/`, `%`, `<<`, `>>` | mathematical result in `U64`/`I64` range; divisor $\neq 0$; shift $< 64$ |
| Termination | each recursive routine and law proof | `variant` strictly decreases in a well-founded order |
| Exhaustiveness | each `match` | every omitted case is unreachable in the branch proof context |
| Law | each `satisfies` | every law of the concept holds |
| `check` | each `check` block | the predicate holds, then joins the active proof context |
| `assume` | each `assume` block | none; joins the proof context and is listed by `gardelc --audit` |

**Structured Diagnostics for AI Agents (`--json-diagnostics`):**
When verification fails, `gardelc --json-diagnostics` emits a deterministic JSON report for automated LLM repair loops containing:
- `contract_tag`: the exact failing label (e.g., `"not_full"`, `"upper_bound"`, or `"overflow_add"`).
- `span`: file, line, and column range of the failing expression or postcondition.
- `path_conditions`: list of active branch conditions and `let` bindings in scope at the failure point.
- `counterexample`: Z3 model values translated back into Gardel record and variant syntax.
- `relevant_lemmas`: in-scope `specify` routines and `ensure` clauses sharing symbols with the failed goal.

---

## 13. Compiler pipeline and Bend v2 / C mapping

The compiler `gardelc` verifies first and erases second, so everything that reaches the backend is total, memory-safe, pure code.

1. **Parse and resolve names.** Enforce selective export and all five visibility rules (§4).
2. **Check types and concepts.** Resolve `Current`, uniform-access dot calls, and `satisfies` obligations.
3. **Generate verification conditions.** Translate representations, ghost code, `let` blocks, contracts, variants, and 64-bit numerics to Boogie, then query Z3.
4. **Erase.** Remove `specify` blocks, laws, contracts, `check`, `assume`, `reveal`, and `variant`.
5. **Monomorphize and closure-convert.** Specialize generic modules and concepts; lift capturing lambdas into first-order environment ADT structs and top-level combinators.
6. **Analyze parallelism and emit Bend v2 / C.** Identify independent recursive `let` bindings for parallel fork-join scheduling and emit either **Bend v2** (`.bend`) or standalone **BendRT C11** (`.c`) for compilation to CPU (Clang/GCC), NVIDIA GPU (CUDA), or Apple GPU (Metal).

| Gardel construct | Bend v2 / BendRT (C11) target |
| --- | --- |
| ADT module (sum / product) | Bend `type` datatype / C tagged struct |
| `match` | `match` / C `switch`, with provably unreachable cases dropped |
| Independent recursive `let` bindings | Parallel fork-join task pair (`a b = f(x) g(y)`) on BendRT rings |
| Tail-recursive routine | Direct C `while` loop / zero-allocation tail jump |
| `U64`, `I64`, `F64`, `CHAR`, `BOOL` | Native 64-bit `U64`/`uint64_t`, `I64`/`int64_t`, `F64`/`double`, `U32`, `U8` |
| First-class closure | Lambda-lifted environment ADT + top-level function |
| Concept use | Monomorphized; zero runtime dictionary overhead |
| Contracts and ghost code | Erased completely |
| `IO[A]` | Host C / Bend v2 IO primitives at `program` boundary |

---

## 14. Worked example: verified parallel tree construction and maximum

This example builds a balanced binary tree over a range `[lo, hi]` in parallel and finds its largest leaf in parallel. Using standard `match`/`if`, `variant`, and `let` bindings, the verifier proves termination, absence of 64-bit overflow, and functional correctness without needing `fold` or `unfold`.

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

What the verifier and compiler do:

1. **`from_range` termination and overflow safety.** When `lo < hi`, `mid = lo + (hi - lo) / 2` satisfies `lo <= mid` and `mid < hi`, so `mid + 1` cannot overflow `U64` and both recursive calls strictly decrease the `variant hi - lo`.
2. **`max_value` structural induction.** In the `Leaf(v)` case, `leaves` is `[v]`, so `v` is both an upper bound and attained. In the `Node(l, r)` case, `l` and `r` are structurally smaller than `Current` (`variant Current`), so the verifier assumes the `ensure` postconditions for `l.max_value` (`ml`) and `r.max_value` (`mr`). Whichever is larger bounds all leaves of `l.leaves.append(r.leaves)` and is a member of one of them.
3. **Parallel execution.** In both `from_range` (`left_tree` and `right_tree`) and `max_value` (`ml` and `mr`), the two recursive `let` bindings are independent, so `gardelc` lowers them to parallel fork-join tasks in Bend v2 / BendRT.

---

## 15. Grammar (EBNF)

Block syntax is locked to `end`-delimited blocks to maximize structural accuracy during LLM code generation and automated proof repair.

```ebnf
File          ::= GroupDecl? ( ModuleDecl | FunctionsDecl | ProgramDecl | ConceptDecl )+

GroupDecl     ::= "group" "{" UpperIdent ( "," UpperIdent )* "}"

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

TypeParams    ::= "[" TypeParam ( "," TypeParam )* "]"
TypeParam     ::= UpperIdent ( ":" UpperIdent )?
Satisfies     ::= "satisfies" UpperIdent ( "," UpperIdent )*
                    ( "where" UpperIdent "satisfies" UpperIdent )?
Refines       ::= "refines" UpperIdent ( "," UpperIdent )*

VisList       ::= "{" UpperIdent ( "," UpperIdent )* "}"
RepresentationSection ::= "representation" VisList ( FieldDecl+ | VariantDecl+ )
FieldDecl     ::= LowerIdent ":" Type
VariantDecl   ::= "|" UpperIdent ( "(" FieldDecl ( "," FieldDecl )* ")" )?
CreateSection ::= "create" VisList RoutineDecl+
FeatureSection::= "feature" VisList RoutineDecl+
SpecifySection::= "specify" VisList ( RoutineDecl | LawDecl )+

RoutineDecl   ::= LowerIdent TypeParams? Params? ":" Type
                    RequireBlock? VariantBlock?
                    "do" Block
                    EnsureBlock?
                  "end"
Signature     ::= LowerIdent Params? ":" Type
Params        ::= "(" Param ( "," Param )* ")"
Param         ::= LowerIdent ":" Type

LawDecl       ::= "law" LowerIdent Params
                    RequireBlock? TriggerBlock? VariantBlock?
                    ( "do" Block )?
                    EnsureBlock?
                  "end"

RequireBlock  ::= "require" TaggedPred+
EnsureBlock   ::= "ensure" TaggedPred+
InvariantBlock::= "invariant" TaggedPred+
VariantBlock  ::= "variant" Expr ( "," Expr )*
TriggerBlock  ::= "trigger" Expr ( "," Expr )*
TaggedPred    ::= LowerIdent ":" Expr

Block         ::= ( Stmt ( "\n" | ";" )+ )* Expr
Stmt          ::= "let" LowerIdent ( ":" Type )? "=" Expr
                | CheckBlock
                | AssumeBlock
                | "reveal" LowerIdent

Type          ::= UpperIdent TypeArgs?
                | Type "->" Type
                | "(" ( Param ( "," Param )* )? ")" "->" Type RequireBlock? EnsureBlock?
TypeArgs      ::= "[" Type ( "," Type )* "]"

Expr          ::= Literal | LowerIdent | UpperIdent Args? | Expr "." LowerIdent Args? | LowerIdent Args
                | Expr BinOp Expr | UnOp Expr | "(" Expr ( "," Expr )* ")"
                | "if" Expr "then" Block "else" Block "end"
                | MatchExpr | LambdaExpr | WithIoExpr
                | RecordLit | RecordUpdate | QuantExpr
Args          ::= "(" ( Expr ( "," Expr )* )? ")"
MatchExpr     ::= "match" Expr ( "case" Pattern "then" Block )+ "end"
Pattern       ::= "_" | LowerIdent | Literal
                | UpperIdent ( "(" Pattern ( "," Pattern )* ")" )?
                | "(" Pattern ( "," Pattern )* ")"
LambdaExpr    ::= "fn" Params ":" Type "do" Block "end"
WithIoExpr    ::= "with" "IO" ( IoStmt ( "\n" | ";" )+ )* Expr "end"
IoStmt        ::= LowerIdent "<-" Expr | Expr
RecordLit     ::= "{" LowerIdent "=" Expr ( "," LowerIdent "=" Expr )* "}"
RecordUpdate  ::= "{" Expr "with" LowerIdent "=" Expr ( "," LowerIdent "=" Expr )* "}"
QuantExpr     ::= ( "forall" | "exists" ) LowerIdent ":" Type "." Expr
CheckBlock    ::= "check" TaggedPred+ "end"
AssumeBlock   ::= "assume" TaggedPred+ "end"

UpperIdent    ::= [A-Z] [A-Za-z0-9_]*      (* Types, Modules, Concepts, Variant Constructors *)
LowerIdent    ::= [a-z] [a-z0-9_]*        (* Routines, Parameters, Variables, Fields, Contract Tags *)
Literal       ::= Integer | Float | String | Char | Bool | "()"
```

---

## 16. Open questions and roadmap

The table below records the architectural decisions locked in v0.4 alongside the remaining items for experimentation.

| Question | Decision / Default in v0.4 | Status |
| --- | --- | --- |
| Block syntax | `end`-delimited blocks | **Locked in v0.4** (optimal for LLM patching) |
| Execution backend | Bend v2 / BendRT (portable C11 + CUDA + Metal) | **Locked in v0.4** (replaces HVM2) |
| Recursion primitives | Standard `match` + `variant` + parallel `let` | **Locked in v0.4** (`fold`/`unfold` removed) |
| Closure semantics | Unrestricted via compile-time closure conversion | **Locked in v0.4** (replaces affine rule) |
| Verifier backend | Boogie and Z3 (with Phase 0 Dafny prototype) | Active |
| Memory management in C | Perceus reference counting with in-place reuse vs. BendRT arena | To benchmark in Phase 1 |
| Gradual verification | None; `assume` audited by `--audit` / `--strict` | **Locked in v0.4** (failure-as-value `RESULT[E, A]` standardized with full combinators and verified algebraic laws) |
| Strings | UTF-8 byte-buffer backed by `SEQ[U64]` with ghost `LIST[CHAR]` | Planned for Phase 4 |

**Roadmap**

1. **Phase 0, feasibility.** Translate the core subset (`U64`, ADTs, `let`, `match`, contracts) to Dafny for verification and to Bend v2 / C11 for execution. Done when `TREE.from_range` and `TREE.max_value` (§14) verify and run in parallel on CPU and GPU (CUDA/Metal).
2. **Phase 1, front end and C/Bend emitter.** Parser, name resolution, visibility rules 1–5, closure conversion, and direct Bend v2 / C codegen.
3. **Phase 2, concepts.** `concept`, `satisfies`, law proofs, triggers, and monomorphization.
4. **Phase 3, direct verifier and AI tooling.** Direct Boogie generation, `--json-diagnostics` with counterexample translation, fuel, and `reveal`.
5. **Phase 4, standard library.** `SEQ`, `BIGNAT`, strings, `IO`, packaging.