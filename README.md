# Gardel

> **A pure, total, statically verified programming language combining Tony Hoare's axiomatic semantics with automated SMT proofs and deterministic parallelism.**

---

## Overview

> 📖 **Comprehensive Language Guide**: Read [`guide/GUIDE.md`](guide/GUIDE.md) for an in-depth reference and tutorial designed for human engineers and AI coding agents.
> 🌟 **Interactive Guided Tour**: Open [`tutorial/index.html`](tutorial/index.html) in your browser for a visual, interactive walkthrough of contracts, SMT verification, fuel theories, and multi-target compilation.

**Gardel** is designed for high-integrity systems, human readability, and seamless AI-assisted software construction. It eliminates traditional runtime failure modes—such as null pointer dereferences, data races, out-of-bounds access, integer overflow, and infinite recursion—by moving correctness guarantees entirely into compile-time mathematical proofs.

### Key Pillars

1. **Axiomatic Semantics (Hoare Logic & Contracts)**:
   Grounded in Tony Hoare's axiomatic basis of computer programming ($\{P\}\;C\;\{Q\}$), every module defines an Abstract Data Type (ADT) with preconditions (`require`), postconditions (`ensure`), and class `invariant` clauses. Routine queries use `Current` and `Result` with uniform access and selective export (`{NONE}`, `{ANY}`, `{FRIENDS}`).
2. **Proofs, Not Runtime Checks**:
   Contracts are discharged statically by SMT solvers (Boogie / Z3) and **erased** at runtime. Verified code executes with zero runtime assertion overhead.
3. **Pure, Total & Deterministic**:
   All functions are pure transformers and proven to terminate (via well-founded `variant` expressions). By Church-Rosser confluence, evaluation order never alters computation results.
4. **Automatic Parallelism**:
   Independent divide-and-conquer bindings run in parallel across CPU cores through C11 and OpenMP, without locks, mutexes, or data races. A Bend v2 file is also emitted, the intended route to GPUs, but no emitted file has been executed yet.
5. **Safe 64-Bit Numerics**:
   Runtime arithmetic uses native 64-bit machine types (`U64`, `I64`, `F64`), while contract logic reasons in unbounded mathematical integers (`MATH_INT`). Every checked arithmetic operation must be statically proven not to overflow.
6. **Linear I/O Firewall**:
   Side effects are strictly isolated to `program` modules using linear `IO[A]` actions (`with IO ... end`). Pure computational modules remain mathematically clean and fully verifiable.

---

## Inspired By

Gardel stands on the shoulders of three foundational language paradigms:

* **Eiffel**: The elegant, human-readable syntax for Design by Contract (`require`, `ensure`, `invariant`), the Uniform Access Principle, `Current` and `Result` conventions, selective export visibility, and contract inspection tools (`--inspect` / `short` view).
* **Dafny**: Automated SMT-backed formal verification, inductive reasoning and ghost specifications (`specify`), termination metrics (`variant`), and proof-erased code generation.
* **Bend**: Deterministic parallelism without locks, from divide-and-conquer recursion. Bend v2 is an intended backend, with CUDA and Apple Metal as the goal.

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
│   └── gardel_language_specification_draft_v0_5.md
├── scripts/               # Setup and verification tool installers
│   ├── install_tools.ps1  # Windows PowerShell installer (Dafny, Z3, Boogie)
│   └── install_tools.sh   # Linux/macOS Bash installer
├── examples/              # Standalone Gardel (.gdl) example programs
│   ├── bounded_stack.gdl
│   ├── echo_server/           # TCP echo server with a proven connection limit
│   ├── greeter.gdl
│   ├── reactive_clock/        # Countdown: a reactive program with timers and a proven state
│   ├── reactive_game/         # A small terminal game with a proven board
│   ├── reactive_hello.gdl
│   ├── peano_ord.gdl
│   └── tree.gdl
├── stdlib/                # Standard Prelude sources, one module per file (embedded into gardelc)
│   ├── concepts/          # EQ, CANONICAL_EQ, ORD
│   ├── core/              # OPTION, RESULT, IO_ERROR, and the reactive types
│   ├── collections/       # LIST_OPS, SEQ
│   ├── numeric/           # BIGNAT
│   ├── text/              # STRING, BYTES, STRING_ERROR
│   └── functional/        # FUNCTION_OPS
├── gardelc/               # The self-contained Gardel Rust compiler
│   ├── Cargo.toml
│   └── src/               # Parser, name resolution, Boogie/Z3 verifier & code generators
└── SETUP.md               # Tooling setup and workflow commands
```

---

## Getting Started with `gardelc`

`gardelc` is a self-contained Rust binary that embeds the standard library and verification toolchain integration.

### Prerequisites

* [Rust 1.81+](https://www.rust-lang.org/)
* *(Optional for direct verification & `--run`)*: [Boogie](https://github.com/boogie-org/boogie), and [Z3](https://github.com/Z3Prover/z3). Boogie is the only verifier; Dafny is installed only as a convenient source of a bundled Z3

#### Automated Tools Setup Scripts

Automated setup scripts are provided in `scripts/` to install Boogie and Z3 (via a Dafny distribution, used only for its bundled Z3):

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

# Verify the standard library sources in ../stdlib/ (all 18 files, linked as one project)
cargo run -- --stdlib

# Inspect the public contract interface of all modules (short view)
cargo run -- --stdlib --inspect

# Inspect a specific module interface
cargo run -- --stdlib --inspect SEQ

# Inspect contracts on an example file
cargo run -- ../examples/bounded_stack.gdl --inspect

# Compile and run an example program
cargo run -- ../examples/greeter.gdl

# Write the embedded standard library to build/stdlib/ (or --emit-stdlib <DIR>); never overwrites edited files
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
