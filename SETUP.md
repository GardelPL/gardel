# Project Setup & BeautifulRust Commands

This document records the exact steps and commands used to set up the `gardelc` project and integrate the [BeautifulRust](https://github.com/morganlinton/BeautifulRust) agent skills and lint configurations.

---

## 1. Project Initialization

Create the new Rust binary project:

```powershell
# From the workspace root: c:\home\Learn\Gardel
cargo new gardelc
```

---

## 2. BeautifulRust Installation

### Option A: Agent Skills (Antigravity / Gemini / Shared `.agents`)

Clone the skills and copy them into the workspace `.agents/skills/` directory:

```powershell
# Create skills destination directory
New-Item -ItemType Directory -Force -Path "c:\home\Learn\Gardel\.agents\skills"

# Clone BeautifulRust temporarily
git clone --depth 1 https://github.com/morganlinton/BeautifulRust.git BeautifulRust-tmp

# Copy skills to .agents/skills/
Copy-Item -Recurse -Force "BeautifulRust-tmp\skills\*" "c:\home\Learn\Gardel\.agents\skills\"

# Clean up temporary clone
Remove-Item -Recurse -Force "BeautifulRust-tmp"
```

The installed skills are:
- `rust-best-practices`
- `rust-review`
- `rust-port`

### Option B: Alternative CLI Installers

Using Gemini CLI:
```powershell
gemini skills install https://github.com/morganlinton/BeautifulRust --path skills/rust-best-practices
gemini skills install https://github.com/morganlinton/BeautifulRust --path skills/rust-review
gemini skills install https://github.com/morganlinton/BeautifulRust --path skills/rust-port
```

Using Node (`npx skills`):
```powershell
npx skills add morganlinton/BeautifulRust
```

---

## 3. Configure Clippy and Project Lints

### 3.1 Copy `clippy.toml`

Copy BeautifulRust's `clippy.toml` to the project root (next to `Cargo.toml`):

```powershell
# From BeautifulRust repository lints/clippy.toml:
Copy-Item "lints/clippy.toml" "gardelc/clippy.toml"
```

Content of `gardelc/clippy.toml`:
```toml
# Flag any function with a bool parameter
max-fn-params-bools = 0

# Allow unwrap, expect, and printing in tests
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-print-in-tests = true
```

### 3.2 Add `[lints]` to `Cargo.toml`

Append the following configuration to `gardelc/Cargo.toml`:

```toml
[lints.rust]
unsafe_op_in_unsafe_fn = "warn"          # each unsafe op needs its own unsafe block
missing_debug_implementations = "warn"   # every public type implements Debug
missing_docs = "warn"                    # [lib] document every public item
unreachable_pub = "warn"                 # pub items that aren't actually exported
rust_2018_idioms = { level = "warn", priority = -1 } # 2018-edition idioms

[lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }

# Panics: return errors, and give every intentional panic a reason.
unwrap_used = "warn"                     # use ?, or expect("why this cannot fail")
expect_used = "allow"                    # expect with a reason is the sanctioned form
panic_in_result_fn = "warn"              # a fn returning Result shouldn't also panic
missing_panics_doc = "warn"              # document reachable panics under # Panics
missing_errors_doc = "warn"              # document failures under # Errors

# Suppressions must explain themselves
allow_attributes = "warn"                # prefer #[expect] over #[allow]
allow_attributes_without_reason = "warn" # every suppression says why

# Unsafe must be justified.
undocumented_unsafe_blocks = "warn"      # // SAFETY: comment on every unsafe block
multiple_unsafe_ops_per_block = "warn"   # one unsafe operation per block

# API shape.
fn_params_excessive_bools = "warn"       # bool parameters: use an enum or two methods
struct_excessive_bools = "warn"          # many bool fields: use an enum or bitflags
clone_on_ref_ptr = "warn"                # write Rc::clone(&x) so cheap clones are visible
redundant_clone = "warn"                 # a clone whose original is never used again
rc_buffer = "warn"                       # Rc<String> / Rc<Vec<T>>: use Rc<str> / Rc<[T]>

# Idioms.
string_add = "warn"                      # build strings with format! or push_str, not +
wildcard_imports = "warn"                # no glob imports (use super::* in tests is allowed)

# Output and debugging leftovers.
dbg_macro = "warn"
todo = "warn"
print_stdout = "warn"                    # [lib] libraries log, they don't print
print_stderr = "warn"                    # [lib] libraries log, they don't print

# Casts that silently lose information.
as_conversions = "warn"                  # prefer From / TryFrom
cast_possible_truncation = "warn"
cast_sign_loss = "warn"

# Pedantic exceptions
module_name_repetitions = "allow"
must_use_candidate = "allow"
module_inception = "allow"
enum_glob_use = "allow"
```

---

## 4. Verification & Common Workflow Commands

Run these inside `gardelc`:

```powershell
# Move into the project directory
cd gardelc

# Check code syntax and types
cargo check

# Run Clippy with BeautifulRust rules
cargo clippy

# Strict check for CI (treat warnings as errors)
cargo clippy --all-targets --all-features -- -D warnings

# Format code according to style rules
cargo fmt --check
cargo fmt

# Run tests
cargo test

# Build and run the project
cargo run
```
