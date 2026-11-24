# Logic, SAT/SMT Solvers, and `rlimit` in Gardel

These notes cover the logical foundations behind Gardel's static verification pipeline (`gardelc` -> Boogie -> Z3): **propositional calculus**, **predicate calculus**, **soundness vs. completeness**, and why **`rlimit`** is used instead of wall-clock timeouts.

---

## 1. Propositional Calculus (Propositional Logic)

**Propositional calculus** deals with statements (**propositions**) that evaluate to either `True` or `False`, combined using boolean connectives:

- **Atomic propositions:** $P, Q, R$ (indivisible boolean facts).
- **Connectives:** `not` ($\neg$), `and` ($\land$), `or` ($\lor$), `implies` ($\rightarrow$), `=` ($\leftrightarrow$).
- **Semantics:** Every atomic proposition is assigned `True` or `False` (a *valuation*), and truth tables determine the value of the compound formula.
- **Decidability:** Propositional logic is **decidable**. With $n$ atomic propositions, there are $2^n$ possible valuations. Checking whether a formula is satisfiable (**SAT**) is NP-complete, but always terminates in finite steps.

### In Gardel

Quantifier-free contracts and simple integer/boolean checks live close to propositional logic (plus decidable arithmetic). For example, in `gardelc/examples/bounded_stack.gdl`:

```gardel
  push(x: A): BOUNDED_STACK[A]
    require
      not_full: Current.count < Current.capacity
    do
      ...
    ensure
      count_incremented: Result.count = Current.count + 1
      capacity_unchanged: Result.capacity = Current.capacity
    end
```

Here, there are no quantifiers ($\forall, \exists$). The verification condition combines boolean structure (`require` implies `ensure`) with quantifier-free linear integer arithmetic.

---

## 2. Predicate Calculus (First-Order Logic / FOL)

Propositional logic cannot express properties over *all* elements of a collection or *some* index in a sequence. **Predicate calculus** extends propositional logic with:

- **Variables and Constants:** $x, k, 0, \text{Current}$ ranging over a domain (e.g., `NAT`, `LIST[A]`, heap locations).
- **Functions:** `xs.length`, `LIST_OPS.at(xs, k)`, `k + 1`.
- **Predicates:** `k < xs.length`, `xs.is_empty`.
- **Quantifiers:**
  - **Universal ($\forall$):** `forall k: NAT. P(k)` — "$P(k)$ holds for every $k$."
  - **Existential ($\exists$):** `exists k: NAT. P(k)` — "$P(k)$ holds for at least one $k$."

- **Undecidability:** Unlike propositional logic, general first-order logic over infinite domains (such as `NAT` or `INT`) is **undecidable** (Church–Turing theorem). No algorithm can take an arbitrary quantified formula and always halt with a `Valid` or `Not Valid` answer.

### In Gardel

Quantifiers appear whenever we specify properties across sequences, lists, arrays, or pointwise functions. For example, in `gardelc/stdlib/text/bytes.gdl`:

```gardel
  law same_items[A](xs: LIST[A], ys: LIST[A])
    require
      same_length: xs.length = ys.length
      fits: xs.length < 18446744073709551615
      same_at: forall k: NAT. k < xs.length implies LIST_OPS.at(xs, k) = LIST_OPS.at(ys, k)
```

---

## 3. Soundness and Completeness

In logic and automated reasoning, we compare what a system **proves/reports** ($\vdash$) against what is **actually true** ($\models$):

- **Soundness:** If the system says something is true/provable, it **actually is** true. *(No false positives; never claims an invalid program is verified.)*
  $$\text{If } \vdash \varphi \text{, then } \models \varphi$$
- **Completeness:** If something **actually is** true, the system **can prove/find it**. *(No missed proofs.)*
  $$\text{If } \models \varphi \text{, then } \vdash \varphi$$

*(Note: Gödel's Completeness Theorem states that standard FOL has a sound and complete proof calculus—every valid formula has a finite proof—but finding that proof automatically is still undecidable in general.)*

---

## 4. SAT vs. SMT Solvers

### SAT Solvers (Propositional Logic)
A SAT solver takes a propositional formula (usually in CNF) and asks whether a satisfying truth assignment exists.
- **Sound:** `SAT` means the returned assignment truly satisfies the formula; `UNSAT` means no such assignment exists.
- **Complete:** Because the search space ($2^n$) is finite, a SAT solver (given enough time and memory) always terminates with `SAT` or `UNSAT`.

### SMT Solvers (Satisfiability Modulo Theories)
An **SMT solver** like **Z3** combines a SAT core with **theory solvers** (linear/nonlinear arithmetic, arrays/maps, bitvectors, uninterpreted functions) and **quantifier instantiation**.

- **Soundness is non-negotiable:**
  - `SAT` means a valid counterexample/model exists in the combined theories.
  - `UNSAT` means no counterexample exists.
- **Completeness depends on the fragment:**
  - **Decidable fragments** (e.g., quantifier-free linear arithmetic, bitvectors, uninterpreted functions): Z3 is **sound and complete**, always returning `SAT` or `UNSAT`.
  - **Undecidable fragments** (e.g., formulas with `forall`/`exists` or nonlinear integer arithmetic): No solver can be simultaneously sound, complete, and always terminating. Z3 **preserves soundness and sacrifices completeness**, returning **`UNKNOWN`** when its heuristics cannot decide.

| Solver / Fragment | Sound | Complete | Possible Outputs |
| :--- | :---: | :---: | :--- |
| **SAT** (Propositional) | Yes | Yes | `SAT`, `UNSAT` |
| **SMT** (Decidable quantifier-free theories) | Yes | Yes | `SAT`, `UNSAT` |
| **SMT** (Quantifiers / Nonlinear arithmetic) | Yes | No | `SAT`, `UNSAT`, `UNKNOWN` |

---

## 5. How Boogie & Z3 Verify Gardel Programs

When `gardelc` verifies a module:

1. **VC Generation (Boogie):** Gardel lowers routines, `require`, `ensure`, `invariant`, and `variant` clauses into a **Verification Condition (VC)** $\varphi$ in Boogie.
2. **Proof by Refutation (Z3):** Boogie asks Z3 whether $\neg \varphi$ (the *existence of a counterexample*) is satisfiable:
   - **`UNSAT`** $\implies$ No counterexample exists $\implies$ $\varphi$ is valid (**verified**).
   - **`SAT`** $\implies$ Z3 found a counterexample $\implies$ bug in code or missing contract/invariant.
   - **`UNKNOWN`** $\implies$ Z3 could not finish or decide $\implies$ verification fails (soundness is preserved: Gardel never treats `UNKNOWN` as verified).

### Why Quantifiers Cause `UNKNOWN`
To reason about `forall` without enumerating infinite domains, Z3 uses **E-matching** (pattern-based instantiation using *triggers*). Two common failure modes happen:
1. **Missing instantiation:** Z3 doesn't see the right pattern to instantiate the `forall`, so it lacks the fact needed to reach `UNSAT`.
2. **Matching loops:** Instantiating a `forall` rule produces a new term that triggers the same rule again indefinitely (e.g. $f(x) \to f(g(x)) \to f(g(g(x))) \dots$), causing the solver to spin without converging.

---

## 6. Why `rlimit` Is Used Instead of Wall-Clock Timeouts

If verification only used a wall-clock timeout (e.g., "stop after 5 seconds"), verification results would be **non-deterministic**:
- A proof might pass on a fast developer laptop (`4.2s`) and fail on a busy CI machine (`5.1s`).
- CPU speed, thermal throttling, and OS scheduling would make builds flaky.

Instead, `gardelc` supports `--z3-rlimit` (passed to Boogie as `/rlimit:<N>`, alongside `--z3-seed` for deterministic randomness):

- **What `rlimit` measures:** Z3's internal **resource counter**—a deterministic count of solver work (SAT decisions/conflicts, theory propagations, quantifier instantiations, etc.), **not** elapsed wall-clock time.
- **Deterministic & reproducible:** Given the same VC, Z3 version, and seed, Z3 performs the exact same number of steps on every machine. If a proof passes under an `rlimit` locally, it will pass under that `rlimit` in CI.
- **Controlled incompleteness:** When Z3 hits a matching loop or a combinatorial blowup, `rlimit` cleanly halts the search and reports `UNKNOWN` (resource limit exceeded). This signals that the proof needs a structural fix—such as splitting a `law`, strengthening a `require`/loop `invariant`, or adding an explicit intermediate assertion (`check` / helper call like `same_items(xs.tail, ys.tail)`)—rather than relying on a faster CPU.
