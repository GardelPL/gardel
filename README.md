# Gardel

> **A pure, total, statically verified programming language combining Tony Hoare's axiomatic semantics with automated SMT proofs and deterministic parallelism.**

---

## Overview

**Gardel** is designed for high-integrity systems, human readability, and seamless AI-assisted software construction. It eliminates traditional runtime failure modes—such as null pointer dereferences, data races, out-of-bounds access, integer overflow, and infinite recursion—by moving correctness guarantees entirely into compile-time mathematical proofs.

### Key Pillars

1. **Axiomatic Semantics (Hoare Logic & Contracts)**:
   Grounded in Tony Hoare's axiomatic basis of computer programming ($\{P\}\;C\;\{Q\}$), every module defines an Abstract Data Type (ADT) with preconditions (`require`), postconditions (`ensure`), and class `invariant` clauses. Routine queries use `Current` and `Result` with uniform access and selective export (`{NONE}`, `{ANY}`, `{FRIENDS}`).
2. **Proofs, Not Runtime Checks**:
   Contracts are discharged statically by SMT solvers (Boogie / Z3 / Dafny) and **erased** at runtime. Verified code executes with zero runtime assertion overhead.
3. **Pure, Total & Deterministic**:
   All functions are pure transformers and proven to terminate (via well-founded `variant` expressions). By Church-Rosser confluence, evaluation order never alters computation results.
4. **Automatic Parallelism via Bend v2 & C11**:
   Independent expressions and divide-and-conquer bindings automatically execute in parallel across CPU cores and GPUs (CUDA / Metal) without locks, mutexes, or data races.
5. **Safe 64-Bit Numerics**:
   Runtime arithmetic uses native 64-bit machine types (`U64`, `I64`, `F64`), while contract logic reasons in unbounded mathematical integers (`MATH_INT`). Every checked arithmetic operation must be statically proven not to overflow.
6. **Linear I/O Firewall**:
   Side effects are strictly isolated to `program` modules using linear `IO[A]` actions (`with IO ... end`). Pure computational modules remain mathematically clean and fully verifiable.

---

## Inspired By

Gardel stands on the shoulders of three foundational language paradigms:

* **Eiffel**: The elegant, human-readable syntax for Design by Contract (`require`, `ensure`, `invariant`), the Uniform Access Principle, `Current` and `Result` conventions, selective export visibility, and contract inspection tools (`--inspect` / `short` view).
* **Dafny**: Automated SMT-backed formal verification, inductive reasoning and ghost specifications (`specify`), termination metrics (`variant`), and proof-erased code generation.
* **Bend**: Massive, deterministic parallel execution, interaction-net and task-ring runtime concepts (BendRT), divide-and-conquer parallelism, and seamless targeting of multi-core CPUs, CUDA, and Apple Metal GPUs.

---

## Code Example: `BOUNDED_STACK[A]`

```eiffel
module BOUNDED_STACK[A]

representation {NONE} -- Opaque internal representation
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

## Repository Structure

```text
├── README.md              # Project overview and introduction
├── spec/                  # Gardel language specifications
│   └── gardel_language_specification_draft_v0_4.md
├── scripts/               # Setup and verification tool installers
│   ├── install_tools.ps1  # Windows PowerShell installer (Dafny, Z3, Boogie)
│   └── install_tools.sh   # Linux/macOS Bash installer
├── examples/              # Standalone Gardel (.gdl) example programs
│   ├── bounded_stack.gdl
│   ├── peano_ord.gdl
│   └── tree.gdl
├── gardelc/               # The self-contained Gardel Rust compiler
│   ├── Cargo.toml
│   ├── src/main.rs        # Parser, type checker, SMT verifier & code generators
│   └── stdlib/            # Extracted Phase 4 standard library bundle
└── SETUP.md               # Tooling setup and workflow commands
```

---

## Getting Started with `gardelc`

`gardelc` is a self-contained Rust binary that embeds the standard library and verification toolchain integration.

### Prerequisites

* [Rust 1.81+](https://www.rust-lang.org/)
* *(Optional for direct verification & `--run`)*: [Boogie](https://github.com/boogie-org/boogie), [Z3](https://github.com/Z3Prover/z3), and [Dafny](https://github.com/dafny-lang/dafny)

#### Automated Tools Setup Scripts

Automated setup scripts are provided in `scripts/` to install Dafny (with bundled Z3) and Boogie:

* **Windows (PowerShell)**:
  ```powershell
  pwsh scripts/install_tools.ps1
  ```
* **Linux / macOS (Bash)**:
  ```bash
  chmod +x scripts/install_tools.sh
  ./scripts/install_tools.sh
  ```

#### Manual / Package Manager Installation

* **Windows** (via `winget` and `dotnet`):
  ```powershell
  winget install Microsoft.Dafny
  dotnet tool install --global Boogie
  ```
* **macOS** (via Homebrew):
  ```bash
  brew install dafny z3 boogie
  ```
* **Linux** (via `apt` and `dotnet`):
  ```bash
  sudo apt install z3
  dotnet tool install --global Boogie
  # Download Dafny from https://github.com/dafny-lang/dafny/releases
  ```

### Running the Compiler

From the `gardelc` folder:

```bash
cd gardelc

# Compile and verify the embedded standard library (SEQ, BIGNAT, STRING, OPTION, GREETER)
cargo run -- --stdlib

# Inspect the public contract interface of all modules (short view)
cargo run -- --stdlib --inspect

# Inspect a specific module interface
cargo run -- --stdlib --inspect SEQ

# Inspect contracts on an example file
cargo run -- ../examples/bounded_stack.gdl --inspect

# Compile and run natively
cargo run -- --stdlib --run

# Emit the embedded standard library to standalone .gdl files
cargo run -- --emit-stdlib
```

### Running Tests

```bash
cd gardelc
cargo test
```

---

## License

MIT / Apache 2.0
