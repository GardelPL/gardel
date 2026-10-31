# Gardel Standard Library: One File or Multiple Files?

## Recommendation

**I would keep the standard library logically modular, but I would not keep it physically confined to one file as Gardel grows.**

For Gardel, I recommend a **one-module-per-file convention, with group files reserved for modules that genuinely need to be compiled and verified together**.

There is an important distinction here:

- **Logical organization:** How types, operations, concepts, and contracts are divided into modules.
- **Physical organization:** How those modules are distributed across source files.
- **Compilation and verification boundaries:** Which modules must be analyzed together to establish correctness.

These are three separate design decisions. Gardel should avoid coupling them unnecessarily.

Your current implementation has a good starting point: `stdlib.gdl` declares a group containing `SEQ`, `BIGNAT`, `STRING`, and `OPTION`, and the compiler embeds the standard library as a source string. The specification also explicitly says that mutually recursive types belong in a single group file, which is a useful mechanism for handling genuine dependencies. gardel_language_specification_d…

The question is whether every standard-library module benefits from that arrangement. I don't think it does.

Gardel has some distinctive characteristics that make this decision particularly important:

1. **Design by Contract:** Public routines expose proof obligations, and library correctness depends on more than successful compilation.
2. **Algebraic data types and invariants:** A module defines a data type, its operations, and its invariant.
3. **Pure, total functions:** Modules can be reasoned about compositionally when their interfaces are sufficiently expressive.
4. **Concepts and laws:** Generic modules may depend on concepts and their verified laws.
5. **AI-driven development:** Small, well-defined modules provide useful units of implementation, verification, and review.
6. **Parallel compilation:** Independent modules could eventually be verified and compiled separately.

These properties favor **small, independently verifiable modules with explicit dependencies**, rather than a single physical source file containing the entire library.

---

## 1. Option A: Keep the entire standard library in one group file

For example:

```text
stdlib.gdl
├── OPTION[A]
├── SEQ[A]
├── BIGNAT
└── STRING
```

This is essentially your current approach.

### Pros

| Advantage | Why it matters for Gardel |
|---|---|
| Simple compiler integration | The compiler can parse one source unit, avoiding the need for a library loader and dependency resolver. |
| Straightforward verification | Related modules can be processed together, and shared definitions are available in the same source unit. |
| Easy distribution | A single file can serve as the initial standard library for the compiler, examples, and demonstrations. |
| Simple versioning | The standard library and compiler can evolve together without managing multiple source artifacts. |
| Convenient for prototypes | You can add a type, modify its contracts, and test the whole library without first building module-resolution infrastructure. |
| Fewer dependency-management decisions | You don't need to establish a package format, import syntax, or dependency-locking mechanism yet. |

The most significant advantage is **engineering simplicity**.

Gardel is still evolving its language semantics, compiler, verification pipeline, and code-generation backends. Every additional compiler subsystem introduces implementation and maintenance costs.

If a single file helps you iterate quickly, there is a good reason to retain it during the early stages.

### Cons

| Disadvantage | Why it matters for Gardel |
|---|---|
| Poor scalability | As the library grows, navigating and reviewing unrelated modules becomes harder. |
| Larger change surface | A small change to `OPTION` lives in the same source artifact as changes to strings, sequences, and large-number arithmetic. |
| Coarse-grained verification | If the compiler processes the whole file as one unit, a local change may require verifying unrelated modules again. |
| Reduced dependency clarity | Physical proximity can obscure which modules actually depend on one another. |
| Less effective parallel development | Different contributors or AI agents modifying the same file are more likely to create conflicts. |
| Weaker reuse boundaries | Reusing one module in another project may require extracting it from the larger file. |
| More difficult testing organization | It becomes harder to establish focused test suites, verification fixtures, and regression checks for individual components. |

There is an additional concern specific to Gardel.

**A large source file should not become a substitute for an explicit module interface.**

Because Gardel uses contracts as proof obligations and module invariants as part of type semantics, the public interface is particularly important. A module should expose enough information for clients to establish their proof obligations without unnecessarily exposing its implementation.

The specification already establishes a foundation for this approach through visibility-controlled interfaces and opaque representations. gardel_language_specification_d…

Splitting the library into files will not automatically improve those properties. It will, however, give the compiler and developers a better opportunity to enforce them.

---

## 2. Option B: One source file per module

For example:

```text
stdlib/
├── option.gdl
├── seq.gdl
├── bignat.gdl
└── string.gdl
```

Each file would define its own module, with the compiler responsible for locating dependencies.

### Pros

**1. Better alignment with Gardel's module model**

Gardel's specification establishes the principle of one module per ADT, with `functions` modules available for collections of free functions. gardel_language_specification_d…

One file per module makes this architecture visible in the source tree.

A developer looking for `OPTION` knows where its definition lives. An AI coding agent working on `STRING` can focus on that module instead of editing a large shared file.

**2. Better incremental verification**

Suppose you modify `STRING` while leaving `OPTION` unchanged.

With independent module compilation, Gardel could reuse the verified artifacts for `OPTION` and other unaffected dependencies.

However, this benefit requires more than splitting files: Gardel needs reliable dependency tracking, interface fingerprints, and a cache of verification results.

A source file is not automatically a verification boundary.

**3. Better parallel development**

Different contributors or AI agents could work on `SEQ`, `STRING`, and `BIGNAT` independently.

This reduces merge conflicts and makes changes easier to review. It also enables CI to test independent modules concurrently.

**4. More precise dependency management**

The compiler can eventually distinguish between:

- A module's public interface.
- Its implementation.
- The modules whose interfaces it depends on.
- The verification results invalidated by a change.

This is especially valuable in a language where contracts, generic constraints, and concept laws contribute to correctness.

**5. More flexible library distribution**

A project might need `OPTION` without needing string manipulation or large-number arithmetic. Separate modules make it easier to package, inspect, and reuse individual components.

That does not require Gardel to support downloading arbitrary third-party packages immediately.

### Cons

**1. The compiler becomes more complicated**

Your current parser explicitly recognizes a `group` declaration and then parses the concepts and modules that follow it. The AST represents the group as a list of module names. main

Moving to independent source files raises additional questions:

- How does the compiler find a module?
- How does a module declare dependencies?
- How are duplicate definitions detected?
- How are circular dependencies diagnosed?
- Which files constitute one compilation unit?
- How are dependencies located across different library directories?

These are manageable problems, but they are real engineering work.

**2. Cross-module verification needs a precise interface model**

Consider a generic module that uses another module's operations in its contracts.

The compiler must know the relevant signatures, preconditions, postconditions, invariants, and applicable concept laws.

It cannot simply load a function's implementation and assume that this is sufficient for compositional verification.

For Gardel, the public verification interface must be well-defined.

**3. Mutual recursion becomes harder to organize**

Suppose two ADTs genuinely depend on each other.

If they are in separate files, the compiler needs a mechanism to recognize that they form a mutually dependent group. Otherwise, it risks introducing a circular-import problem.

This is a strong argument for retaining groups as a language feature, even if most standard-library modules move to individual files.

**4. Incremental verification requires cache correctness**

A cached proof result is only safe to reuse if the assumptions under which it was established remain valid.

Changes to a public contract, invariant, concept law, or solver configuration may invalidate downstream verification results even if the implementation of a dependent module remains unchanged.

The build system must track these relationships.

**5. More files mean more metadata and tooling**

You may eventually need a manifest, module search paths, dependency graphs, interface files, and compiler diagnostics that identify the correct source file.

These facilities are worthwhile, but they should be introduced when the compiler needs them, rather than merely because multiple files look cleaner.

---

## 3. The most important distinction: source files versus verification units

I would not make the decision solely based on file size.

For Gardel, I would distinguish three cases:

| Case | Recommended organization | Reason |
|---|---|---|
| Independent ADT | One module per file | Clear ownership and potential for independent verification. |
| Mutually recursive ADTs | One group containing multiple modules | The compiler must reason about their interdependent definitions and invariants. |
| Related free functions | A `functions` module, grouped by responsibility | Functions can share a coherent interface without pretending to define an ADT. |

The key principle is that **a group should represent a genuine compilation or verification relationship, not merely a convenient collection of library modules**.

For example, putting `OPTION`, `SEQ`, `BIGNAT`, and `STRING` in one file does not necessarily mean that all four must be verified as one indivisible unit.

Conversely, splitting mutually recursive types into separate files does not make them independently verifiable.

Gardel's specification already provides a sensible starting point: a module defines one ADT, and mutually recursive types are declared in a group file. gardel_language_specification_d…

I would preserve that distinction.

---

## 4. My proposed architecture for Gardel

I would adopt a **modular standard library with explicit group boundaries**.

For example:

```text
gardel/
├── compiler/
│   └── ...
│
├── stdlib/
│   ├── core/
│   │   ├── option.gdl
│   │   ├── result.gdl
│   │   ├── tuple.gdl
│   │   └── bool_ops.gdl
│   │
│   ├── collections/
│   │   ├── list.gdl
│   │   ├── seq.gdl
│   │   └── set.gdl
│   │
│   ├── numeric/
│   │   ├── bignat.gdl
│   │   ├── int_math.gdl
│   │   └── rational.gdl
│   │
│   ├── text/
│   │   ├── string.gdl
│   │   └── string_builder.gdl
│   │
│   └── io/
│       ├── io.gdl
│       └── ...
│
├── examples/
│   ├── hello_world.gdl
│   ├── collections.gdl
│   └── verified_arithmetic.gdl
│
└── tests/
    ├── core/
    ├── collections/
    └── numeric/
```

This is an illustrative layout, not a recommendation to implement every listed module now.

In particular, I would not create `RESULT`, `SET`, `STRING_BUILDER`, or other modules until their designs and use cases justify them.

### A. Keep `OPTION` independent

`OPTION[A]` represents the presence or absence of a value. Its implementation does not inherently require `SEQ`, `BIGNAT`, or `STRING`.

It should be independently compilable and verifiable.

### B. Give `SEQ` its own source file

`SEQ` is more interesting because it is a balanced binary-tree representation designed to support parallel computation.

Its implementation and contracts deserve focused attention, particularly for:

- Structural recursion and termination.
- The relationship between the tree and its logical sequence.
- Length preservation.
- Correctness of concatenation and splitting.
- Parallel execution and its semantic guarantees.

A dedicated file makes it easier to review these properties without navigating unrelated modules.

### C. Keep `BIGNAT` separate from the primitive numeric types

Your specification describes `BIGNAT` as a library type implemented using `SEQ[U64]`, with `MATH_INT` as its ghost model. gardel_language_specification_d…

This is a good example of why the dependency graph matters.

Conceptually:

```text
U64, MATH_INT
       |
       v
      SEQ
       |
       v
     BIGNAT
```

`BIGNAT` depends on the sequence abstraction, but `SEQ` should not depend on `BIGNAT`.

This is a conceptual dependency diagram, not a claim that the current compiler already supports these dependency boundaries.

### D. Treat `STRING` as a separate design decision

String handling will likely become one of the more substantial parts of the library.

Its eventual design may involve Unicode semantics, indexing, slicing, parsing, and conversions.

It should have its own module and tests rather than growing inside a general-purpose group.

The underlying representation can change later without forcing unrelated modules to change, provided the public interface and contracts remain compatible.

---

## 5. What should happen to `group`?

**Keep it, but give it a narrower purpose.**

I would reserve `group` for modules that must be compiled and verified together because of mutual recursion or another explicitly defined language-level requirement.

For example:

```eiffel
group { TREE, TREE_NODE }

module TREE
    -- ...
end

module TREE_NODE
    -- ...
end
```

This is schematic syntax; the exact declarations would depend on the actual recursive relationship.

The important property is that the group identifies a meaningful boundary.

I would avoid using:

```eiffel
group { SEQ, BIGNAT, STRING, OPTION }
```

merely to indicate that these modules belong to the standard library.

**Library membership and mutual-recursion groups are different concepts.**

The standard library should be a collection of modules. A group should be a compilation construct.

If Gardel eventually needs a package or library manifest, that should be a separate mechanism.

For example, a future manifest might express:

```text
library Gardel.Core

modules:
    OPTION
    SEQ
    BIGNAT
    STRING
```

This is illustrative manifest syntax, not proposed Gardel language syntax.

Such a manifest could describe library membership and dependencies without changing the meaning of `group`.

---

## 6. One important issue in the current implementation

There is a distinction between the organization of `stdlib.gdl` and how the compiler obtains its contents.

Your compiler currently contains a function named `phase4_stdlib_source()` that returns the standard-library source as a Rust string. The compiler implementation therefore has a second representation of the standard library: the source file itself and the embedded source string.

This is acceptable for a prototype, but it becomes inconvenient as the library grows.

For example, changes to the library could require keeping the following synchronized:

1. The canonical `.gdl` source.
2. The source embedded in the compiler.
3. The tests and examples that exercise the library.

That creates an unnecessary maintenance burden.

### My recommendation

Move toward a single canonical source of truth:

```text
stdlib/
    option.gdl
    seq.gdl
    bignat.gdl
    string.gdl
```

Then make the compiler load those files through its standard-library resolution mechanism.

For early development, that mechanism could be as simple as a known standard-library directory. You do not need a full package manager.

There is one implementation caveat: the compiler currently parses a source file into a `FileAst` containing a group, concepts, and modules. Supporting independently loaded modules will require the compiler to assemble the appropriate compilation units before verification and code generation.

I would not implement a complex dependency resolver prematurely. Start with a deterministic module-loading mechanism and introduce dependency analysis as needed.

---

## 7. Recommended migration strategy

I would make this change incrementally.

| Phase | Change | Goal |
|---|---|---|
| 1 | Extract the embedded standard-library source into canonical `.gdl` files. | Eliminate duplicate source representations. |
| 2 | Make the compiler load the standard library from those files. | Establish a single source of truth. |
| 3 | Support independent modules and explicit module dependencies. | Separate library organization from compilation units. |
| 4 | Retain group compilation for mutually recursive modules. | Preserve the existing capability without making every module part of a group. |
| 5 | Add dependency-aware incremental verification. | Reuse verified results when dependencies and relevant interfaces have not changed. |
| 6 | Add per-module tests and verification regression suites. | Improve confidence in contracts and invariants as the library grows. |

I would prioritize the first three phases. Incremental verification is valuable, but it should follow a reliable module and interface model.

### One additional design requirement: preserve reproducibility

Because Gardel aims for deterministic compilation and verification, module loading should be deterministic too.

In practice:

- Resolve a module name to a unique source file.
- Reject duplicate module definitions.
- Detect dependency cycles that are not explicitly supported through groups.
- Make dependency order independent of filesystem enumeration order.
- Include relevant compiler, solver, and dependency-interface information in verification-cache keys.

These requirements will become especially important if independent modules are compiled concurrently.

---

## 8. Final decision

Here is how I would evaluate the alternatives specifically for Gardel.

| Criterion | One group file | One file per module | Modular library + explicit groups |
|---|---:|---:|---:|
| Initial implementation simplicity | Excellent | Moderate | Moderate |
| Long-term maintainability | Low to moderate | High | **High** |
| Incremental verification potential | Limited | High | **High** |
| Support for mutual recursion | Excellent within the group | Requires additional mechanisms | **Excellent** |
| Parallel development | Limited | High | **High** |
| Clear dependency boundaries | Limited | High | **High** |
| Compatibility with Gardel's ADT model | Good | Excellent | **Excellent** |
| Fit for an evolving language | Good initially | Good | **Best overall** |

### My conclusion

I would **not discard the current single-file approach as a mistake**. It is a reasonable prototype architecture, especially while Gardel's compiler and semantics are evolving.

However, I would not make it the permanent architecture of the standard library.

The best long-term design is:

1. **One module per file** as the default convention.
2. **One group per genuine mutually recursive compilation unit.**
3. **Explicit dependencies** between modules.
4. **Public contracts and concept laws as the basis for compositional verification.**
5. **A standard-library loader** that reads canonical source files rather than embedding the entire library in the compiler.
6. **Incremental verification** once the module and interface boundaries are stable.

The most important architectural principle is this:

> **Gardel should organize its standard library around verification boundaries and semantic dependencies, not around the convenience of placing every definition in one file.**

That approach preserves the simplicity of your current design where it is useful while creating a clear path toward a larger, independently verifiable standard library.