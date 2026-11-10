# Changelog

All notable changes, theoretical soundess fixes, verification overhauls, front-end language additions, backend code generation improvements, and standard library alignments for the Gardel programming language and `gardelc` compiler.

---

## [Unreleased]

Spec draft v0.5. [plan/Roadmap.md](plan/Roadmap.md) holds the detail for each task.

### Language and runtime
- **`Connect` and `CancelTimer`** (F.7). A reactive program can open TCP connections: `Connect(tag, host, port)` is answered by `Connected(tag, conn)` or `ConnectFailed(tag, err)`, and the connection then works as an accepted one. The connect never blocks the loop; `host` is an IPv4 address or `localhost`, with no name lookup; and a connect that takes more than 10 s, or `GARDEL_CONNECT_TIMEOUT_MS`, fails. `CancelTimer(id)` clears a pending timer. Scripts and logs have forms for all four, so a run with a connect replays. `tests/connect.rs` covers a connect that succeeds, one refused, one timed out and hosts that are not addresses, over real TCP on Windows. The POSIX code has not been run yet.
- **A law of a routine whose body is hidden reaches its calls in a body** (F.6). A routine of another module, such as `MAP.get` in a client, was a different function at each fuel, joined only by a step between two fuels. Its laws speak of it at the default fuel, and a routine that calls it, `find(id) do Current.items.get(id) end`, unfolds to a call one fuel lower, so `remove(id).find(id)` was not known to be `None`. Such a routine is now one function at every fuel, where its body is hidden. `tests/hidden_body_fuel.rs` checks it.
- **`grest` 0.4.1: the state is proven to be the handler's** (F.6). `REST_SERVER.completed(c, data)` is the request that bytes complete on a connection. `on_bytes` ensures that the next state is the one the handler of that request gave back, or the same when there is none, and `serve` ensures `moves_to(e, Result)`, a ghost query that says this for every event. Before, only a scripted run checked it.
- **`examples/todo_api` 0.1.1** (F.6). `TODO_STORE`'s `add`, `replace` and `remove` ensure what `find` gives after them: the added todo is found, the replaced one has its new `done`, the removed id is not found, and every other id is as it was. A client proves these through the contracts, and a claim that a delete removes every todo is refused.
- **`grest` 0.4.0: handlers with state** (F.6). `REST_ROUTE[S]`, `REST_ROUTER[S]` and `REST_SERVER[S]` take the type of a service's state. A handler is `(s: S, r: REST_REQUEST) -> REST_OUTCOME[S]`, the next state with its response, and `on(m, p, h)` adds one. `on_json` handlers are also given the body, `on_view` adds a `GET` route that reads the state, and `on_get`, `on_post`, `on_put` and `on_delete` keep handlers that do not look at it. `REST_SERVER.start(args, port, routes, s0)` starts from `s0`, and each complete request moves the server to the state its handler gave back. `serve`'s safety contract is unchanged. The predicates over commands moved to `REST_COMMANDS`, as they do not depend on the state, and `REST_ROUTER.max_routes` is gone. A breaking change: `--compare-with` against 0.3.0 reports 42 breaking changes and 8 additions. `examples/hello_api` 0.3.0 has a `NAT` state that stays `0`.
- **`examples/todo_api`**, create, read, replace and delete over JSON, on `grest` 0.4.0 (F.6). Its state is a `TODO_STORE` of `MAP[NAT, TODO]`, whose invariant, proven from `MAP`'s laws, says that each todo is kept under its own id and that no todo has an id at or past the next, so an id is never given twice, even after a delete. `tests/todo_api.rs` has a client prove the invariant through the contracts and refuses three claims past it, then runs a session of 14 requests over real sockets.
- **Lambdas over a type parameter verify** (F.6). `fn(s: S, r: REST_REQUEST): REST_OUTCOME[S] do ... end` inside `REST_ROUTE[S]` was emitted as its body, outside any binder of `s`, and the module was `E0199` (`undeclared identifier: s`). Such a lambda is now lifted to a generic Boogie function, as other lambdas are, and its calls carry its type.
- **A type parameter was seen in any parameter whose type held its letter** (F.6). The check for a parameter that no argument mentions compared text, so `S` was found in `REST_REQUEST`, and `REST_ROUTE[S].plain(m, p, h: (r: REST_REQUEST) -> HTTP_RESPONSE)` got an untyped trigger Boogie refused. Types are now compared by name.
- **Names that Boogie reserves** (F.6). A parameter, `let`, `match` binder, lambda parameter or quantified variable named `old`, `var`, `type`, `function`, `int` or another of Boogie's words that Gardel does not reserve was a parse error in the emitted program, and the module was `E0199`. Each is now renamed in its routine before the verifier sees it.
- **A `match` binder of a number type is in its range** (F.6). In `case Some(id) then s.find(id)` on an `OPTION[NAT]`, `id` was not known to be a `NAT` in the proof, so no fact of `find(id)` guarded by `id`'s range applied.
- **A generic module can hold functions over its type parameter, and clients can chain its calls** (F.6). Two faults in the Boogie translation stopped `ROUTER[S].empty.on(1, peek).on(2, add)` from verifying. Boogie 3.4.3, under the type encoding `gardelc` uses, states the result type of a generic function with an axiom whose guards shift by one after an `int` or `bool` parameter, so for `on(k: NAT, h: (s: S, n: NAT) -> OUTCOME[S])` no axiom fired on a call that had another call as its argument. `gardelc` now moves scalar parameters after the others in every generic Boogie function and its calls. And the contract of a routine whose type parameter only its result fixes, such as `ROUTER[S].empty`, was stated of `ROUTER[int]` alone; its calls in that axiom now carry the result type. `tests/generic_handlers.rs` has a client prove facts of such a chain and run it in C.
- **`grest` 0.3.0: `REST_SERVER`** (F.6). A REST service is now its handlers, its routes and two lines, `init` returning `REST_SERVER.start(args, 8090, routes)` and `step` returning `s.serve(e)`. `start` refuses a route whose pattern is not valid, or an argument that is not a port, with exit status 2 before it listens. `serve`'s contract proves, for every state and event, that the limit of 64 connections holds and never changes, that an event about a connection does nothing, closes it, or sends it one answer and then closes it, and that any other event names no connection. `examples/hello_api` 0.2.0 moves to `grest`: four routes, one with a parameter, and no connection loop of its own. Its answers now carry a `Content-Type`, and its errors are JSON objects.
- **`grest` 0.2.0: requests, responses, and routes that hold their handlers** (F.6). `grest` now depends on `ghttp` and `gjson`. A `REST_ROUTE` holds a method, a pattern such as `/users/:id`, and its handler, a pure function `(r: REST_REQUEST) -> HTTP_RESPONSE`. `REST_ROUTER.dispatch` answers a request: a target that cannot be read is a `400`, the first matching route answers (`HEAD` by a `GET` route), and otherwise `405` or `404`, each error a JSON object. `REST_REQUEST` gives `param`, `query`, `header` and `json_body`, and `REST_RESPONSE` gives `json`, `text` and status helpers. A route added with `on_json` answers a body that is not JSON with a `400` before its handler runs. A client proves the statuses through the contracts, and `grest/tests/cases.txt` sends 32 requests to a nine-route API and compares 4 more answers byte for byte as sent.
- **`ghttp` 0.4.0: `Content-Type`, and every status code on the wire** (F.6). `HTTP_RESPONSE.with_content_type` states `text/plain; charset=utf-8` or `application/json`, from a fixed set, so the serializer's head stays exact and holds no injected line. `HTTP_STATUS.code_digits` sent `500` for any code it did not list, so `HTTP_STATUS.make(409)` went out as `500`; it now sends the code. Responses that state no type are sent as before.
- **A `let` bound to a `match` expression** has the type of its arms in the proof (F.6). `let s = match a case Some(x) then x case None then "" end` declared `s` as an integer, and the module was `E0199`.
- **`grest` 0.1.0, the URL layer** (F.6), a new package. `URL_TEXT.decode` does percent decoding, with `+` as a space in a query. `REQUEST_TARGET.parse` splits a target into decoded path segments and query fields, at most 16 and 32, and refuses a target with more. `PATH_PATTERN` matches `/users/:id`, and a match is proven to give exactly one value per parameter. `grest/tests/cases.txt` holds 101 cases, which `cargo test` runs in C.
- **`match` binders of one name and different types** (F.6). `case Some(e)` in one arm and `case Error(e)` in another declared one local with two types in the proof, and the module was `E0199`. A binder that reuses a name already bound in its routine, by a parameter, a `let` or another binder, is now renamed in its arm.
- **`STRING.make_empty` builds in C** (F.6). It compiled to a constructor that does not exist in C, and `gcc` failed. It is now the static empty string, as the capacity is a fact of the proof only.
- **A routine's name is a function value** (F.6). `apply_small(halve, 50)` means `apply_small(fn(x: NAT): NAT do halve(x) end, 50)` and is checked as that lambda is, against a refined type included. It was `E0199` (`undeclared identifier`). A local or parameter of the same name hides the routine, and a generic routine or one of another module is not converted.
- **String literals no longer slow each other down** (F.6). Each literal's definition was a ground axiom of the module, so every proof in it, each literal's own lemma included, paid for all of them: 30 route paths in a module next to `ghttp` took 530 s and ran out of resources 56 times. A definition is now reached only through its own lemma, and the same module verifies in 76 s with no error, `ghttp` included. Other proofs know a literal through the lemma's facts, as before. `--compare-with` keeps the plain definitions.
- **`LIST.length` takes constant time in C** (F.6). Each cell keeps the length of the list it starts. A loop that tests `length` on every step over 50,000 items went from 1.8 s to under 0.12 s. `MAP` and `NUMBER_TEXT.parse_digits`, which test `length`, are no longer quadratic.
- **A list declared `LIST[NAT]` links in C** (F.6). `let e: LIST[NAT] = LIST.empty` followed by `e.prepend(1)` called `LIST_NAT__prepend`, but the runtime is emitted as `LIST_U64`, and `gcc` failed to link. A list method now calls the runtime by its normalized name.
- **Refined function types are enforced** (F.12). In `(x: NAT) -> NAT require small: x < 100 ensure bounded: Result <= x`, the `require` is proven at each application, and the `ensure` is proven of every function passed, by the caller, and assumed in the body. A lambda passed to such a parameter may use its `require`. On a record field, an `ensure` is an invariant of the record, and on a result an `ensure` of the routine. Before, both clauses were ignored. Clauses that nothing would enforce, in a `let`, a lambda, a type argument or a nested function type, are `E0321`, and a parameter with a `require` that is bound to a name or returned is `E0322`.
- **`let a = f(k)` on a function parameter `f`** has `f`'s result type in the proof (F.12). It had the module's type, and the module was `E0199`.
- **`gjson`** (F.4), a verified JSON package: an opaque `JSON` value whose invariant keeps text valid UTF-8, keys unique and in order, and nesting at most 64 levels; a total parser with byte offsets in every error and a 4 MiB limit; and a writer with no white space and keys in order. The law `JSON.scalar_round_trip` proves `parse(v.to_bytes) = Ok(v)` for scalars and strings. Arrays and objects are covered by tests (`gjson/tests/cases.txt`).
- **A field of a machine number type is in range** in the proof (F.4), for sum types and records.
- **Substitution avoids capture** (F.4): a `match` binder, quantified variable, lambda parameter or `let` in a `require` no longer captures a variable of the call's arguments.
- **`NUMBER_TEXT.digits_step`** (F.4), a law with no trigger giving one step of `digits`.
- **`MAP[K, V]`** (F.3), a prelude module: `empty`, `get`, `has`, `put`, `remove`, `size`, `keys`, `values` and `fold`, for any ordered key. Its representation is hidden, and its laws say what `get` gives after `put` and `remove` and that `keys` ascend. Each law has a trigger, so a client proves facts such as `m.put(1, x).get(2) = m.get(2)` with no call. `KEY_ORDER[K]` holds the ascending-list predicates. It runs in C with `STRING` keys and frees what it allocates.
- **Function types of three parameters** (F.3), such as `(acc: B, k: K, v: V) -> B`, in the proof, in lambdas and in C. Before, the verifier knew one and two, and a third gave an invalid Boogie type.
- **A law of another module states its hypotheses at their types** (F.3). `not j.equals(k)` on a bounded `K` was emitted as `OPTION`'s `equals`, and the client was `E0199`.
- **`let m: MAP[STRING, NAT] = ...` builds in C** (F.3). The C backend compared the declared type with the value's as text, and `NAT` inside a type argument did not match `U64`.
- **The limit of a `require` that calls a function** is in the GUIDE (section 9): a recursive function with one cannot be unfolded a step deeper. Task F.13 fixes the cause.
- **`ghttp` 0.3.0: header names ignore case, and `Content-Length` is computed** (F.5 follow-up). `HTTP_HEADERS.lookup_header` finds `Content-Type` when a client sends `content-type`, and the law `found_after_add` proves it for any spelling. `HTTP_RESPONSE` works out `Content-Length` from the body and promises that it reads back as the body's length, so a response can no longer declare a length its body does not have. `NUMBER_TEXT.digit_count` gains bounds on its result.
- **`ghttp` 0.2.0 parses what the bytes say** (F.5). The parser state keeps the bytes it has received, and each `feed` parses all of them again, so a request cut across TCP reads gets the answer the whole request gets. Before, a split `GET /nope` was answered `200` as `GET /`. A request is complete when its framing says so: the head ends at the first blank line and the body is `Content-Length` bytes. A `POST` body now arrives. The parser reads the request line, the version and each header field. It refuses conflicting `Content-Length` values, both framings at once and a body on a safe method, answers chunked encoding with `501`, and returns what follows a request as `Complete`'s remainder. `HTTP_FRAMING.faithful` states what a result promises, and every parse step proves it: a waiting state holds every byte received, the remainder is the tail of those bytes, and the body is what comes just before it. `examples/hello_api` gains `POST /echo`. `ghttp/tests` is now a client package, and its 27 cases in `cases.txt` each run whole and cut at every byte. `cargo test` runs them and probes `hello_api` over real sockets with split requests and bodies.
- **Text and bytes operations** (F.2). `STRING` gains `slice`, `find`, `find_char`, `starts_with`, `ends_with`, `matches_at`, `trim`, `count_char` and `split`. `BYTES` gains `concat`, `slice`, `matches_at`, `starts_with`, `index_of_byte` and `index_of`. Each contract states its result on `chars` or `model`, and the searches say both that a match is real and that `None` means there is none. A `slice` shares its buffer in C.
- **`STRING satisfies ORD`** (F.2), by the byte order of its UTF-8 form, which on well-formed text is the order of code points. It is a key order, not a collation. A `STRING` is now a type argument for `[K: ORD]`.
- **`BYTES_BUILDER`** (F.2) appends a part at constant cost and joins all of them once, in `to_bytes`. Building 64 KB one byte at a time with `BYTES.concat` is quadratic; with the builder it stays within start-up noise.
- **`NUMBER_TEXT` and `NUMBER_ERROR`** (F.2): `nat_to_text`, `parse_nat`, `i64_to_text`, `parse_i64`, `hex4_text` and `parse_hex4`. A parse is total, and a number too large for its type is `OutOfRange`, never a wrapped value. The laws `nat_round_trip`, `i64_round_trip` and `hex4_round_trip` are proven.
- **`NAT.from_i64` and `I64.from_nat`** (F.2) convert between the machine types, with the obligation `in_range`.
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
- **`forall` and `exists` over a machine type ignored its range** (F.2). `NAT`, `U64`, `I64` and `CHAR` are ranges of the solver's integers, and the quantifier ranged over every integer. So a negative witness could prove `exists k: NAT. ...`. Assumed as a hypothesis, `forall k: NAT. ...` also claimed something about negative numbers. Each quantifier now carries its type's range.
- **A bound was not checked where a type was used** (F.1). `KV_MAP[STRING, NAT]` with `K: ORD` was accepted, and a generic body is proven only for what satisfies its bound. A written or inferred use is now `E0317`, and the C backend checks every instance again.
- **`x.code` was the identity on every receiver**, which is only true of `CHAR`. In a module with a routine `code` that returns its own type, a false `ensure` such as `x.code = x` verified. `code` is now the identity on a `CHAR` alone, and a module's own routine wins over every built-in name (`first`, `head`, `length`, `append`, `at`, `apply` and the rest), which gave `E0199` before. Bend had the same fault: a run that declares a `code` routine now calls it on every receiver.
- **Obligations reach every expression** (D.9). Overflow, division by zero, callee preconditions and `check` were skipped in lambdas, in `if` and `match` nested in an expression, in tuples, under `not`, in record fields and in a body `match`'s scrutinee. Such programs verified, then wrapped or crashed at run time.
- **`I64`** (D.10). `/` and `%` proved Euclidean results while the binary truncated, so `(-7) / 2 = -4` verified and the program computed `-3`. `-x` at `I64.min` and `I64.min / -1` had no check. Wrapping on `I64` was read as unsigned.
- **Private members** (E.0). Private routines and the constructors of a hidden representation were reachable from other modules, so a client could forge a value of an opaque type.
- **Laws and private routines across modules** (E.0, E.2). A law with a hidden `require` reached clients with no hypothesis, so `check absurd: False` verified. It is now a Rule 1 error. A private routine reached through a generic result, such as `o.get.secret`, passed `E0113` and was called with its `require` unchecked, so a verified program could crash. Name resolution now follows those receivers, and the verifier refuses a private target and any `require` it cannot state.

- **`assume` under `--strict`** (E.3). An `assume` nested in an expression, such as a `let` value or a call argument, was neither refused by `--strict` nor listed by `--audit`, so a false contract could verify at exit 0. Every expression and contract clause is now covered.

### Verifier
- **`--compare-with` called a move from `EQ` to `ORD` a breaking change** (F.2). A concept kept through a refinement is not lost, so `STRING` gaining `ORD`, which refines `EQ`, is now an addition. Against the prelude before F.2, the check reports 0 breaking changes and 36 additions.
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
- `ghttp` 0.3.0 changes its API. `HTTP_RESPONSE.ok_text`, `created_text` and `make_text` take no length text: drop the second argument. `--compare-with` against 0.2.0 reports these 3 breaking changes and 6 additions. `lookup_header` now matches names that differ only in case, so code that relied on an exact match may find more.
- `ghttp` 0.2.0 changes its API (F.5). `HTTP_PARSER_STATE.make_partial`, `can_accept`, `HTTP_REQUEST.make_from_wire`, and `HTTP_PARSER.parse_method_prefix`, `scan_target_length` and `parse_target_at` are gone. `--compare-with` against 0.1.0 reports these 8 breaking changes and 11 additions. `HTTP_REQUEST.make` takes no separate body length, and `target` is now the target as received, not `"/"`. `HTTP_ERROR` gains `BodyNotAllowed`, so a `match` on it needs that case. A request line with no version, which the old parser answered as `GET /`, is now `MalformedRequestLine`. `hello_api` builds unchanged apart from its new route.
- A `forall` or `exists` over `NAT`, `U64`, `I64` or `CHAR` now ranges over that type only (F.2). A proof that rested on a value outside it no longer goes through. Every example, `ghttp` and `hello_api` are unaffected and stay 0/0. Their `.bpl` files change only by the new range guards and by new prelude theory, and their `.c` files only by new runtime helpers.
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

