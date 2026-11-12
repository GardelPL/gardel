# Foreign Interfaces: Design and Plan

Status: proposal. How Gardel calls code written in other languages, and how other languages will later call Gardel.

---

## Part 1. Design

### 1.1 The problem

A Gardel program needs libraries it cannot write itself yet: SQLite, PostgreSQL, cryptography, compression, numeric kernels. Those exist in C and Rust. Calling them is hard for any language with its own memory model, and harder for Gardel, because foreign code can break the guarantees Gardel proves:

- **Purity and determinism.** The verifier assumes a function gives the same result for the same input.
- **Totality.** Every Gardel routine returns.
- **The IO boundary.** Only `program` modules and `IO[A]` routines have effects (spec §11.1).
- **Memory.** Gardel has its own thread-caching allocator and ownership rules (Roadmap C).
- **Untrusted input.** Data from outside is `BYTES` and enters the verified core only through `RESULT`-returning gateways (spec §7, §11.4).

### 1.2 How other languages do it

| Language | Mechanism | Lesson for Gardel |
|---|---|---|
| Eiffel | `external "C" alias "name"`: a routine whose body is C, with Eiffel contracts | Closest to Gardel: contracts on a routine with a foreign body |
| Elixir / Erlang | **Ports**: a separate OS process, messages over pipes. **NIFs**: C in the VM, fast, but a crash takes down the VM | Two paths: isolated and safe, or in-process and fast |
| Java | JNI: C code against a large VM API | A big API surface is costly; avoid |
| Rust | `extern "C"`, `#[no_mangle]`, `cbindgen` | The C ABI is the common denominator, and Rust can provide a safe one |

### 1.3 Decision

Gardel supports **three** mechanisms:

1. **In-process calls over the C ABI**, with **Rust as the first-class language for shims**. This is the fast path for computation and for native libraries.
2. **Isolated worker processes** reached through **reactive commands** (§1.5). This is the safe path for large, effectful or untrusted code in any language.
3. **Native commands** (§1.6): an in-process native service that a `reactive program` reaches through a command and an event. This is how a server uses a database, since a `reactive program` has no `IO` and so cannot call a foreign routine.

Raw unrestricted FFI, NIF-style VM APIs and JNI-style interfaces reduce to one of these or are left out. More mechanisms would add language, verifier and runtime surface without a matching benefit.

**Why the C ABI.** Gardel lowers to one C11 file built with `gcc`. A C-ABI call is a plain C call in that file, with no runtime added. SQLite (`libsqlite3`), PostgreSQL (`libpq`), OpenSSL and zlib all expose C APIs, and most other language bindings call those same APIs.

**Why Rust for shims.** A Rust crate can wrap a C library or a Rust crate (`rusqlite`, `postgres`, `sha2`) behind a small, safe C ABI, so the far side of the boundary is memory safe too. Rust is the documented, tooled path (G.4, G.5). Plain C shims also work; the tests use them so they need only `gcc`.

**First libraries.** SQLite and PostgreSQL both start on the C-ABI path, through Rust shims over `rusqlite` and `postgres`. A `program` calls them as effectful routines (G.6). A `reactive program` calls the same shim as a native command (§1.6, G.6b). A worker process is the alternative when a database must be fully isolated, for example in a multi-tenant server.

### 1.4 Requirements on the C-ABI path

These follow from §1.1 and shape the decisions in Part 2.

1. **Pure and effectful are different kinds.** A routine that starts a process, reads a clock or touches global state is an effect and must be typed `IO[A]`. Only deterministic, total, stateless code may be a pure foreign routine.
2. **Contracts are the only facts.** The verifier sees a foreign routine only through its `require` and `ensure`, as Phase E already does for dependencies (bodies withheld, contracts kept: `linked_contract_interface`, Roadmap E.2).
3. **Foreign `ensure` clauses are checked at run time.** A foreign result is outside data. If an `ensure` can be compiled to C, it is checked after each call, so a broken shim fails loudly instead of making proofs unsound. A clause that cannot be compiled must be marked `trusted`, so trust is visible in the source.
4. **Trust is reported.** `gardelc --trust-report` lists every foreign symbol and every `trusted` clause, including dependencies, the way Lean's `#print axioms` does. A "verified 0/0" result then has a stated scope.
5. **Linking is not in the source.** The `.gdl` file names the symbol only. Libraries and crates are declared in `Gardel.toml`.
6. **Data crossing has a fixed ABI and owner.** Memory a shim returns comes from Gardel's allocator. A shim never frees Gardel memory and never keeps a pointer after it returns.
7. **Handles have a lifetime.** A foreign resource (a database connection) is an opaque handle with an explicit close and a release function the runtime calls.
8. **No unwinding across the boundary.** A Rust panic must not unwind into C. Errors the caller handles go through `RESULT`.
9. **Thread safety.** The C backend runs parallel code under OpenMP, so a pure foreign routine can be called from several threads at once.

### 1.5 The isolated path: worker processes as reactive commands

A `reactive program` already talks to the world by messages: it returns `COMMAND`s and receives `EVENT`s (spec §11.6). `CONN_ID` is an opaque handle, `Send`/`Received`/`Closed` move `BYTES`, and failures arrive as data. A worker process is one more kind of connection:

```eiffel
-- COMMAND
| Spawn(tag: NAT, program: STRING, args: SEQ[STRING])   -- answered by Spawned / SpawnFailed
-- EVENT
| Spawned(tag: NAT, conn: CONN_ID)                      -- then Send / Received / Close as for TCP
| SpawnFailed(tag: NAT, err: IO_ERROR)
| Exited(conn: CONN_ID, code: I64)
```

The worker's stdin and stdout carry the bytes, encoded with `gjson` or another format. The worker can be written in any language. A crash arrives as `Closed` and `Exited`. This needs no new syntax, no verifier rules and no trusted contracts: the bytes are untrusted input that goes through the Invariant Gateway. Isolation is a real process, not an annotation. Planned as G.8.

### 1.6 Native commands: foreign effects for reactive programs

A `reactive program` has no `IO` (spec §11.6, rule 2), so it cannot call a foreign routine of either kind in G-D1. Every server in the repository is reactive: `grest`, `todo_api` and `api_gateway`. Without a third path, the SQLite shim of G.6 could be used only by a `program` that runs once and ends. A worker process (§1.5) would work, but it costs a second executable and a wire format for something that can be linked in.

A native command is the in-process form of §1.5. The program names a service and sends bytes. The host calls the service on a helper thread and delivers the answer as an event:

```eiffel
-- COMMAND
| Native(tag: NAT, service: STRING, request: BYTES)    -- answered by NativeReply
-- EVENT
| NativeReply(tag: NAT, res: RESULT[IO_ERROR, BYTES])
```

A service is one C-ABI symbol, declared in `Gardel.toml` (G-D7). Its rules:

- **No blocking.** The call runs on a helper thread, as console reads do, so a slow query never holds up timers, sockets or a `Shutdown`.
- **One thread per service, requests in order.** A service may keep state, such as an open database, and needs no locking of its own. Two services run side by side.
- **No handle crosses.** A connection or a prepared statement lives inside the service. The program refers to it by a number in its request bytes, so requirement 7 of §1.4 does not apply.
- **The reply is untrusted input.** It is `BYTES`, and it enters the verified core through a `RESULT`-returning gateway, as a `Received` payload does. There is no `ensure`, no `trusted` clause and no new verifier rule.
- **Replay stays exact.** The command and its reply are logged. A scripted run and a replay read the reply from the script or the log and call nothing, so a recorded session with a database replays with no database.
- **A failure is an event.** An unknown service, a non-zero status and a request sent after `Stop` each give `NativeReply(tag, Error(...))`. A crash in the service ends the process, as in G-D6.

What this path does not give: a contract on the service. A pure, fast computation, such as a hash, should stay a pure foreign routine (G-D1), which a reactive program can call like any pure code.

Planned as G.6b.

### 1.7 The other direction: Gardel called from other languages

Gardel already produces C. An `export "C"` mark on routines of a `functions` module could produce a static library and a header, so Rust, Python (`ctypes`/`cffi`), Go (`cgo`) and C# (`P/Invoke`) call verified Gardel code: a JSON validator, an HTTP head parser, a UTF-8 decoder. This brings Gardel into existing codebases one routine at a time. Planned as G.9.

### 1.8 Rejected for now

- **A WebAssembly sandbox** (isolation inside the process). It needs a Wasm runtime embedded in the C backend, a large dependency. Worker processes cover isolation.
- **A `foreign "port"` routine form.** A port call is an effect that can fail and takes time; as a routine it would hide that. §1.5 gets the same result with machinery that already exists.
- **Dynamic loading (`dlopen`/`LoadLibrary`).** Static linking keeps one executable and avoids DLL search paths on Windows.

---

## Part 2. Plan: Phase G, Foreign Routines over the C ABI

### 2.1 Decisions

**G-D1. Two kinds of foreign routine, chosen by the return type.**

| Kind | Return type | Shim must be | Verifier model | Callable from |
|---|---|---|---|---|
| Pure | not `IO` | deterministic, total, no global state, thread-safe, keeps no pointer | uninterpreted function; `require` asserted at the call, `ensure` assumed | anywhere |
| Effectful | `IO[A]` | anything, but must return | an action with no relation between calls; `ensure` assumed on the result | `program` modules and `IO` routines |

A `reactive program` cannot call either directly (it uses no `IO`). It may call a pure foreign routine, as it may call any pure code, but never an effectful one. It reaches foreign effects through commands: a native command for a linked service (§1.6), or a worker process (§1.5).

**G-D2. Syntax.** `foreign "C" "symbol"` replaces `do`.

```eiffel
functions HASH

feature {ANY}

  fnv1a(data: BYTES): U64
    foreign "C" "ghash_fnv1a"
    end

  sha256(data: BYTES): BYTES
    require
      bounded: data.length <= 1_048_576
    foreign "C" "ghash_sha256"
    ensure
      digest_length: Result.length = 32          -- checked after every call
    end

end
```

An effectful routine:

```eiffel
  open(path: STRING): IO[RESULT[IO_ERROR, DB]]
    foreign "C" "gsqlite_open"
    end
```

Code that calls these routines is ordinary Gardel and is verified as usual. A caller of `sha256` must prove `bounded`, for example by checking the size first and returning `Error` when it is too large.

**G-D3. Linking is in `Gardel.toml`.**

```toml
[native]
rust = ["native/hash"]     # Cargo crates with crate-type = ["staticlib"]
c    = ["native/shim.c"]   # C files compiled with the program
link = ["sqlite3"]         # system libraries, passed as -l
```

Static linking only. A dependency's `[native]` section is linked into the projects that use it.

**G-D4. Types that cross (first version).**

| Gardel | C argument | C result |
|---|---|---|
| `U64`, `I64`, `F64`, `BOOL`, `CHAR` | the C type the backend already uses | same |
| `BYTES` | `const uint8_t *ptr, size_t len`, borrowed for the call | `gardel_bytes` built with `gardel_ffi_bytes_new(len)` from `gardel_ffi.h` |
| `STRING` | UTF-8 `ptr, len`, borrowed | not allowed: return `BYTES` and decode with `STRING.from_utf8` |
| `RESULT[E, A]` (result only, `A` crossable, `E` a prelude error) | — | `int32_t` status, `A` through an out pointer |
| opaque handle (G.6) | `void *` | `void *`, freed by a declared release symbol |

Other ADTs, `SEQ`, `LIST` and closures do not cross. Encode them as `BYTES` (for example with `gjson`).

The Rust side of `sha256`, for reference:

```rust
#[no_mangle]
pub extern "C" fn ghash_sha256(ptr: *const u8, len: usize) -> gardel_ffi::BytesOut {
    gardel_ffi::guard(|| {
        let data = unsafe { gardel_ffi::BytesIn::from_raw(ptr, len) };
        gardel_ffi::BytesOut::from_slice(&sha2::Sha256::digest(data.as_slice()))
    })
}
```

**G-D5. `ensure` on a foreign routine.** An `ensure` clause the C backend can compile is checked after each call. A failure stops the program with `foreign contract violated: <module>.<routine>.<label>` and exit code 70. A clause it cannot compile is error `E07xx foreign_ensure_unchecked` unless written `trusted label: expr`. `require` is checked statically at call sites, like any routine.

This is the first contract that runs, so it changes two statements of the spec, and G.0 must amend both:

- **Principle 3 (§2)** says contracts are "discharged by the verifier and erased. Nothing is evaluated at runtime." New text: this holds for every contract on Gardel code. The one exception is an `ensure` on a foreign routine, which states a fact about code the verifier cannot see, and is therefore checked where the foreign value enters.
- **§7.2.1** says "runtime assertion checks and panic handlers do not exist in Gardel binaries." New text: none exist for Gardel code. A program that declares no foreign routine has none at all. A program that declares one has one check per checkable foreign `ensure` clause, and one abort entry (`gardel_ffi.h`).

The doctrine of §7.2 is kept: a failed foreign `ensure` is a bug in the shim, not a condition of the outside world, so it is not a `RESULT`. It cannot be rejected at compile time, so it stops the program at the boundary, before the false value reaches verified code. The table of obligations in spec §12 gains a row, "Foreign ensure: checked after each call, or marked `trusted`".

**G-D6. Failures in the shim.** A panic or abort in a shim ends the process. Rust shims build with `panic = "abort"`, and `gardel_ffi::guard` turns a panic into an abort with a message. Errors the caller should handle go through `RESULT`, never through a crash.

**G-D7. Native commands (§1.6).** A service is declared in `Gardel.toml`, beside the libraries that hold it:

```toml
[native.services]
sqlite = "gsqlite_service"     # service name = C symbol
```

The symbol has one fixed signature, for every service:

```c
int32_t gsqlite_service(const uint8_t *request, size_t len, gardel_bytes *reply);
```

Status 0 gives `NativeReply(tag, Ok(reply))`. Any other status gives `Error(OsError(status))`, and `reply` is ignored. The request is borrowed for the call, and the reply is built with `gardel_ffi_bytes_new`, as in G-D4, so `gardel_ffi_bytes_new` must be safe to call on a helper thread. A `Native` command that names a service the manifest does not declare is answered by an `Error` at once. The compiler cannot check the name, since `service` is a value, so a library should wrap it in one routine that builds the command.

The format of the request and reply bytes belongs to the service. `gjson` is the default choice.

**G-D8. Exit codes of the runtime.** G-D5 adds a code, and the codes the runtime already uses are spread over §11 and §11.6. G.0 puts them in one table in the spec:

| Code | Meaning | Where it is defined today |
|---|---|---|
| any | The program's own code, from `main` or `Stop` | §3, §11.6 |
| 2 | The host refused to start or to continue: an argument that is not UTF-8, an unknown name in `GARDEL_EVENTS`, a log that is not for this program | §11, §11.6 |
| 3 | A replay differed from its log | §11.6 |
| 70 | A foreign contract was violated (`EX_SOFTWARE`) | G-D5, new |

A program can return 2, 3 or 70 itself, so a code does not prove where it came from. The message on standard error does: each runtime exit prints one line that starts with `gardel:`.

### 2.2 Steps

Each step ends with `cargo test` green and `--stdlib --strict` 0/0, as in earlier phases.

#### G.0 Specification
- [ ] Spec §12 "Foreign routines": §1.4 requirements, G-D1 to G-D6, the type table, the shim obligations and the error codes.
- [ ] Amend Principle 3 (§2) and §7.2.1 for run-time checks of foreign `ensure` clauses, and add the row to the obligations table, with the text of G-D5.
- [ ] One table of runtime exit codes (G-D8), referred to from §11 and §11.6.
- [ ] §11.6: `Native` and `NativeReply`, their rules (§1.6), and their scripted and log forms.
- [ ] Guide section with the `HASH` example, a C shim and a Rust shim.
- Acceptance: spec and this plan agree.

#### G.1 Parser and checker
- [ ] Parse `foreign "C" "<symbol>"` as a routine body. `<symbol>` must be a C identifier.
- [ ] Diagnostics, each with a negative test:
  - a type that does not cross (G-D4);
  - a `foreign` routine with `do`, or in a `reactive program`;
  - the same symbol declared twice with different signatures;
  - `trusted` used outside a foreign routine.
- Acceptance: the `HASH` module type-checks.

#### G.2 Verifier
- [ ] Pure foreign: a Boogie function with no body, plus the `ensure` clauses as axioms guarded by `require`. Reuse the E.2 contract-only path (`linked_contract_interface`).
- [ ] Effectful foreign: a procedure that returns a fresh value with the `ensure` clauses as postconditions.
- [ ] Tests:
  - a client proves `sha256(x).length = 32` from the contract;
  - a client cannot prove anything beyond the contract (e.g. `sha256(x) /= x`): rejected, naming the missing fact;
  - two calls of an effectful foreign routine with the same argument cannot be proved equal;
  - a call that breaks `require` is rejected.
- Acceptance: tests pass. No change to existing verification results.

#### G.3 C backend, with C shims
- [ ] `gardelc/runtime/gardel_ffi.h`: `gardel_ffi_bytes_new`, the status codes, the abort entry.
- [ ] Emit an `extern` prototype for each foreign symbol, and lower calls through `runtime_call` (`c11_typed.rs`), marshalling per G-D4.
- [ ] Compile each checkable `ensure` into a check after the call (G-D5).
- [ ] Add `c = [...]` files and `link = [...]` libraries to the `gcc` command (today a fixed argument list in `main.rs`, the `Command::new("gcc")` call).
- [ ] Tests with C shims only, so CI needs `gcc` alone:
  - FNV-1a over `BYTES` gives the known values;
  - a shim that returns 31 bytes from `sha256` stops with the clause name and exit code 70;
  - a `RESULT` shim returns both `Ok` and `Error`;
  - a parallel map calling a pure foreign routine gives the same result as the serial one.
- Acceptance: tests pass on Windows (MinGW `gcc`) and Linux. AddressSanitizer clean, checked by hand as in Roadmap C.

#### G.4 Rust shims in the build
- [ ] `rust = [...]` in `[native]`: run `cargo build --release` in each crate, then link its static library.
- [ ] Windows: the C backend uses MinGW `gcc`, so the Rust target must be `x86_64-pc-windows-gnu`. An MSVC static library does not link with MinGW. `gardelc` passes `--target` itself and, if the target is missing, stops with the `rustup target add` command to run.
- [ ] Add the system libraries a Rust static library needs (`cargo rustc -- --print native-static-libs`: e.g. `ws2_32`, `userenv`, `bcrypt`, `ntdll` on Windows; `pthread`, `dl`, `m` on Linux).
- [ ] Rebuilds are left to Cargo; `gardelc` only runs it.
- Acceptance: a test crate with no dependencies (FNV-1a in Rust) links and gives the same values as the C shim from G.3, on both platforms. Skipped with a message when `cargo` is absent.

#### G.5 The `gardel-ffi` crate and generated bindings
- [ ] `tools/gardel-ffi`: a small Rust crate with no dependencies. Types `BytesIn<'a>` (borrowed slice), `BytesOut` (built through `gardel_ffi_bytes_new`), `Status`, and `guard`.
- [ ] `gardelc ffi-bindings <module>` writes `gardel_<module>.h` and `gardel_<module>.rs`. The `.rs` file has the exact `extern "C"` signatures and a compile-time check, such as `const _: unsafe extern "C" fn(*const u8, usize) -> u64 = ghash_fnv1a;`, so a shim with the wrong signature fails `cargo build`, not at run time.
- [ ] `examples/ffi_hash`: the `HASH` module, a Rust shim using the `sha2` crate, and a program that hashes a file. Verifies 0/0.
- Acceptance: changing a parameter type in the `.gdl` file without updating the shim fails the build with a Rust type error.

#### G.6 Effectful routines and handles: SQLite
- [ ] Opaque handle types: `foreign handle DB release "gsqlite_close"` declares a module with `representation {NONE}` (like `CONN_ID`), crossing as `void *`. The runtime calls the release symbol when the last reference goes, and at exit.
- [ ] An explicit `close(db): IO[UNIT]`. Using a handle after `close` returns `Error(Closed)` from the shim, not undefined behavior: the shim keeps a closed flag.
- [ ] `native/sqlite`: a Rust shim over `rusqlite` with SQLite bundled, so no system library. Gardel module `SQLITE` with `open`, `exec`, `query` (rows returned as `BYTES` and decoded in Gardel) and `close`.
- [ ] `examples/sqlite_notes`: a command-line notes program that adds, lists and deletes rows. Row decoding goes through a `RESULT`-returning gateway.
- Acceptance: the example verifies 0/0 and runs on both platforms. AddressSanitizer clean on Linux with the shim built in debug mode.

#### G.6b Native commands: SQLite for reactive programs (§1.6, G-D7)
- [ ] Prelude: `Native(tag, service, request)` in `COMMAND` and `NativeReply(tag, res)` in `EVENT`, each at the end. Check for name clashes with existing programs, as F.7 did.
- [ ] `[native.services]` in `Gardel.toml`. `gardelc` emits a table of service names and symbols into the program's C file, with an `extern` prototype for each.
- [ ] Host: one helper thread per declared service, started on its first request, with a queue of requests served in order. A reply wakes the event loop, as a console read does. `Stop` waits for a running call for at most 1 second, like the socket queues, and drops queued requests.
- [ ] `gardel_ffi_bytes_new` is safe on a helper thread, with a test that allocates on the helper and frees on the main thread.
- [ ] Scripted and log forms, as every new command needs (Phase D decision 3): events `NativeReply:<tag>:<text>` and `NativeFailed:<tag>:<code>`; command `Native:<tag>:<service>:<text>` in the log, and `native: <tag> <service> <text>` on standard error in a script. A replay calls no service.
- [ ] `native/sqlite` gains `gsqlite_service`, over the same `rusqlite` code as G.6. Requests and replies are JSON: `open`, `exec`, `query` and `close`, with the database named by a number the service gives out.
- [ ] A Gardel package `gsqlite_reactive` (or a module of the G.6 package): one routine per request that builds the `Native` command, and a gateway that decodes a reply into rows, returning `RESULT`.
- [ ] `examples/todo_api` keeps its todos in SQLite, through an `on_await` route (F.8): the request waits under its tag, and `wake` answers it on `NativeReply`.
- [ ] Tests:
  - a C service that echoes its request, so CI needs `gcc` alone;
  - a service that sleeps 2 seconds while a timer ticks every 100 ms: no tick is late;
  - two requests to one service are answered in the order sent;
  - an undeclared service and a non-zero status each give an `Error`;
  - a recorded session of `todo_api` replays with the database file deleted.
- Acceptance: `todo_api` verifies 0/0 under `--strict`, keeps its todos across a restart, and its recorded session replays exactly with no database. The proven invariants of `TODO_STORE` are unchanged.

#### G.7 Trust report and packages
- [ ] `gardelc --trust-report` (also in `--json` output): every foreign symbol, the package that declares it, its kind (pure or effectful), every `trusted` clause, and every native service with its symbol.
- [ ] The report covers dependencies. Adding a dependency with `[native]` shows its entries.
- Acceptance: the report for `examples/sqlite_notes` lists exactly the `SQLITE` symbols. A test adds a `trusted` clause and sees it in the report.

#### G.8 Worker processes through reactive commands (§1.5)
- [ ] `Spawn`, `Spawned`, `SpawnFailed`, `Exited` in `COMMAND`/`EVENT`. The child's stdin and stdout carry the bytes. `Send`, `Received` and `Close` are reused.
- [ ] `examples/port_echo`: a reactive program that drives a Python worker.
- Write `plan/ffi/ports.md` before starting.

#### G.9 Export to C (§1.7)
- [ ] `export "C"` on routines of a `functions` module with crossable types. Output: a static library and a header. The caller is unverified, so the routine's `require` clauses are checked at run time at the entry.
- Write `plan/ffi/export.md` before starting.

### 2.3 Order

G.0 → G.1 → G.2 → G.3 is the core and can ship alone: Gardel calls C. G.4 and G.5 add Rust. G.6 is the first real library. G.6b follows it and reuses its shim; it needs G.3 for linking and G.4 for the Rust build, but not handles, so a C echo service can be built right after G.3 if servers need native code sooner. G.7 should land with or before G.6, because a database shim is the first large body of trusted code. G.8 and G.9 are independent of each other and of G.4–G.7. PostgreSQL follows G.6 with the same pattern over the `postgres` crate.

### 2.4 Risks

- **Windows toolchain.** MinGW `gcc` plus the `-gnu` Rust target is the setup most likely to fail on a new machine. G.4's error message must name the fix. Moving the C backend to `clang` would allow MSVC Rust libraries; that is out of scope.
- **Allocator mismatch.** If a shim returns memory from another allocator, Gardel frees it with its own and the heap breaks. `BytesOut` makes the right way the easy way, and the ASan runs in G.3 and G.6 check it.
- **False confidence.** A foreign routine declared pure that is not deterministic makes proofs unsound in ways no check finds. The guide must say "if in doubt, make it `IO`", and the trust report makes every pure foreign routine visible.
- **`gardelc` has no crate dependencies.** None of this adds any: `gardelc` runs `cargo` and `gcc` as programs. Only `tools/gardel-ffi` and the shims are Rust crates.

### 2.5 Open questions

1. Should `trusted` clauses also be allowed outside foreign routines (for example on lemmas)? This plan says no.
2. Should a pure foreign routine be usable in `ensure`/`specify` (ghost use)? This plan says no: the verifier would rely on code it cannot see, in a place with no run-time check.
3. Exit code 70 (`EX_SOFTWARE`) for a broken foreign contract, or the code used today for other run-time faults? G-D8 proposes 70: codes 2 and 3 already mean something else, and no other run-time fault has a code today.
4. Should a native service get its own helper thread (G-D7), or should all services share a pool? One thread each keeps a service free of locks. A pool would let one slow service delay another.
