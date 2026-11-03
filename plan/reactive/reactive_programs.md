# Proposal: Reactive Programs (explicit non-termination)

Status: **design proposal, not implemented.** Syntax is illustrative and has not been run through `gardelc`. It is written against spec v0.4 and slots into §11 (Effects and IO) next to the existing `program` form. Section 12 lists the exact spec changes it needs.

Revision 2 (2026-11-03) fixes the problems found in review against spec v0.4:
- `step` is now **pure** and returns commands, and the runtime performs them. It no longer runs `IO` itself. This is what makes "each event is handled in finite time" and "deterministic given the event sequence" true (sections 2, 4, 5).
- The program state is an ordinary `module` with its own `invariant`. Invariant preservation now follows from the existing construction-site obligation (§6, §12), so no new verification-condition kind and no anonymous `state` block or record type are needed (sections 3, 4).
- `init` can fail and can ask for resources such as listening sockets (section 3).
- The echo example is rewritten so it verifies. The old version had a `NAT` underflow and also counted rejected connections when they closed (section 7.1).
- Routine layout follows the §15 grammar (`require`, `variant`, `do`, `ensure`, in that order).
- `IO.exit` cannot bypass `Stop`, and `Shutdown` must stop (section 4).
- Partial functions: the old rules made partial routines unreachable, and the "bounded wrapper" in 10.3 could not exist. Both are fixed (section 10).

## 1. Problem

Gardel is pure and total (§2, principle 2). Every routine terminates, there is no `while`/`for`/`loop`, and a `program` runs `main` once and returns an `EXIT_CODE`. That rules out servers, TUIs, games, and daemons, which are correct when they run until told to stop, possibly forever.

Weakening totality in general would break the language's core promise. The goal is the opposite: keep every function total, and put non-termination in **one explicit, visible, verifiable place**, the runtime's event loop.

## 2. Design principles

1. **Totality is unchanged.** Every routine in every module, including `init` and `step` of a reactive program, must terminate. The termination obligation of §12 applies as before.
2. **Non-termination is declared, never implicit.** A program that may run forever says so with the `reactive` keyword. Nothing else in the language can diverge, except routines explicitly marked `partial` (section 10).
3. **The runtime owns the outer loop and all effects.** The programmer writes a pure, total `step(state, event)` that returns the next state plus a list of **commands** (print, send, close, set a timer, and so on). The runtime performs the commands and reports their outcomes back as later events. This is the Elm / "functional core, imperative shell" architecture.
4. **Productivity replaces termination.** The obligation is "every step terminates and yields a result", not "the program terminates". This is the same idea as productive corecursion in Coq/Agda.
5. **Proofs are per step.** The state type's invariant holds after every step, so safety properties ("never more than `limit` live connections") hold for the whole unbounded run by induction.

Why `step` is pure and not `IO`-typed: if `step` could run `IO.read_line` or a socket send, a single step could block forever on the outside world, because totality of Gardel code says nothing about how long an external call waits. The effect would also make the result depend on values that are not in the event log (§11.4 models every read as a fresh unknown value), so replay would not be deterministic. Moving every effect into commands keeps both guarantees, and it means a reactive program contains **no `IO` code at all**.

## 3. Syntax

```eiffel
reactive program NAME

feature {ANY}

  init(args: SEQ[STRING]): STEP[S]
    require
      ...
    do
      ...
    ensure
      ...
    end

  step(s: S, e: EVENT): STEP[S]
    require
      ...
    do
      ...
    ensure
      ...
    end

end
```

- `S` is any ordinary user type, normally a `module` in its own file whose `invariant` states the safety properties. It is inferred from the return type of `init`, and `step` must use the same `S`.
- A `reactive program` has exactly `init` and `step` and no `main`. It may contain other routines (helpers, `specify` lemmas) like any `program`.
- `init` is pure as well. It cannot touch the world directly, but it can return commands (for example `Listen(8080)`) and it can refuse to start with `Stop(code, ...)`, for example when `args` are invalid.

New prelude types (one module per file under `stdlib/core/`, see section 12):

```eiffel
module STEP[S]
representation {ANY}
  | Continue(next: S, cmds: LIST[COMMAND])
  | Stop(code: EXIT_CODE, cmds: LIST[COMMAND])   -- cmds run before exit (e.g. a final Print)
end

module COMMAND
representation {ANY}
  | Print(text: STRING)
  | ReadLine                                      -- ask for one line; answered by a Line event
  | SetTimer(id: NAT, millis: U64)                -- answered by Tick(id)
  | Listen(port: NAT)                             -- answered by Accepted / ListenFailed
  | Send(conn: CONN_ID, data: BYTES)              -- failure answered by SendFailed
  | Close(conn: CONN_ID)
end
```

`EVENT` is defined in section 6. `CONN_ID` is an opaque prelude handle (`representation {NONE}`, `satisfies CANONICAL_EQ, ORD`), so programs can compare and store handles but never forge one.

Commands in one `cmds` list run in list order. A command never blocks the loop. A command that can fail reports its failure as an event, never as a crash, in line with the Error Handling Doctrine (§7.2).

## 4. Typing and verification rules

1. **Step totality.** `init` and `step` are ordinary total routines. Any recursion inside them needs a `variant`. The runtime loop is host code and is not subject to this check.
2. **Invariant establishment and preservation come for free.** Because `S` is a module type, its invariant is part of the type (§6). Every `Continue(next, ...)` that `init` or `step` builds must hold a valid `S`, and every record literal or update that builds that `S` is a construction site with the usual obligation (§12). Inside `step`, the parameter `s: S` is assumed to satisfy the invariant. Together these give "the invariant holds after `init` and after every step" with no new kind of verification condition.
3. **Purity.** `init` and `step` must not have `IO[...]` types and must not contain `with IO`. A reactive program uses no `IO` at all, so the existing IO checks (`io_in_pure_module`, linearity) need no change for it. `IO.exit` therefore cannot appear, and the only way out is `Stop`.
4. **Shutdown must stop.** The verifier adds the obligation `e = Shutdown implies Result is Stop` to `step` (diagnostic `"Phase 4 (Reactive Violation)"`, tag `shutdown_must_stop`). This guarantees a clean exit path that the runtime does not have to force.
5. **Bounded work per step.** Because `step` is pure and total and commands do not block, each event is handled in finite time. The program can only run forever by receiving infinitely many events.
6. **Events are untrusted input.** Payloads arrive as `BYTES`, or as `RESULT[IO_ERROR, STRING]` for lines that were already decoded. They become verified ADTs only through `RESULT`-returning boundary routines (§7.2.2, §11.4). The verifier assumes nothing about event payloads beyond their types.
7. **Entry-point firewall.** A `reactive program`, like a `program`, is a root and cannot be referenced from other modules. Its `step` can still be exercised from test programs if it is factored into a `functions` module (section 7.3).

## 5. What is and is not guaranteed

Guaranteed (given the verifier proves the obligations):
- No crash, null dereference, out-of-bounds access, or overflow in any step.
- The state invariant holds after `init` and after every step, for an unbounded run.
- Each event is handled in finite time by Gardel code.
- **Determinism:** the sequence of states and commands is a function of `args` and the observed event sequence. Logging the events is enough to replay a run exactly, because command outcomes come back as events.
- Every run that receives `Shutdown` ends with the code chosen by `step`.

Not guaranteed (the verifier does not model the outside world, §11.5):
- That events ever arrive, that commands succeed, or how long the runtime takes to perform them.
- The order in which concurrent events arrive. Determinism is relative to the observed order.
- Liveness ("every request is eventually answered"). This needs fairness assumptions and is out of scope for the first version.
- Delivery of `Shutdown` on `SIGKILL` or power loss. `Shutdown` is delivered for SIGINT/SIGTERM only.

## 6. Events

A small, closed set to start with:

```eiffel
module EVENT
representation {ANY}
  | Tick(id: NAT)                                -- a SetTimer(id, _) fired
  | Key(code: NAT)                               -- raw-mode key input (TUI / games)
  | Line(res: RESULT[IO_ERROR, STRING])          -- answer to ReadLine
  | Accepted(conn: CONN_ID)                      -- new connection on a Listen port
  | ListenFailed(port: NAT, err: IO_ERROR)
  | Received(conn: CONN_ID, data: BYTES)
  | SendFailed(conn: CONN_ID, err: IO_ERROR)
  | Closed(conn: CONN_ID)                        -- peer closed, or our Close completed
  | Shutdown                                     -- SIGINT / SIGTERM
end
```

There is no `Started` event, because `init` already plays that role.

Runtime guarantees about handles, which the example relies on:
- Each `CONN_ID` comes from exactly one `Accepted` event.
- After `Close(c)` runs, at most one more event mentions `c`, and it is `Closed(c)`.

Which variants a runtime can deliver depends on which command families it supports, so this list grows in phases (section 9). The type stays closed, and adding a variant is a prelude change, so programs should end their `match` with a `case _` fallback.

## 7. Examples

### 7.1 Echo server with a connection limit

The state lives in its own file, `server_state.gdl`. Only the server may build it directly:

```eiffel
module SERVER_STATE

representation {ECHO_SERVER}
  live: LIST[CONN_ID]
  limit: NAT

invariant
  within_limit: live.length <= limit

end
```

```eiffel
reactive program ECHO_SERVER

feature {ANY}

  init(args: SEQ[STRING]): STEP[SERVER_STATE]
    do
      Continue({ live = LIST.empty, limit = 64 },
               LIST.empty.prepend(Listen(8080)))
    end

  step(s: SERVER_STATE, e: EVENT): STEP[SERVER_STATE]
    do
      match e
        case Accepted(c) then
          if s.live.length < s.limit then
            Continue({ s with live = s.live.prepend(c) }, LIST.empty)
          else
            Continue(s, LIST.empty.prepend(Close(c)))
          end
        case Received(c, data) then
          Continue(s, LIST.empty.prepend(Send(c, data)))
        case Closed(c) then
          let rest = LIST_OPS.filter(s.live, fn(x: CONN_ID): BOOL do x != c end)
          Continue({ s with live = rest }, LIST.empty)
        case ListenFailed(_, _) then
          Stop(1, LIST.empty.prepend(Print("cannot listen on port 8080")))
        case Shutdown then
          Stop(0, LIST.empty)
        case _ then
          Continue(s, LIST.empty)
      end
    end

end
```

What the verifier checks:
- `init`: the literal `{ live = LIST.empty, limit = 64 }` is a construction site, and `0 <= 64` holds.
- `Accepted`: the path condition `s.live.length < s.limit` gives `s.live.length + 1 <= s.limit` at the update.
- `Closed`: `LIST_OPS.filter` ensures `not_longer: Result.length <= xs.length`, and the invariant on `s` gives `rest.length <= s.limit`.
- `Shutdown` returns `Stop`, which discharges rule 4.

Two bugs from the first draft are now impossible. The old `Closed` branch computed `conns - 1` without knowing `conns > 0`, which is a `NAT` underflow. It also counted a `Closed` event for a connection that had been rejected over the limit, so the count could drift below the true number of live connections. Tracking the live set removes both, and connections that were rejected never enter `live`.

The invariant also bounds the state: the connection list can never grow past `limit`, so this server cannot leak memory through connection bookkeeping (see open question on resource bounds).

### 7.2 Turn-based game or TUI loop

The state is the board, a module whose invariant states that the position is legal. `step` handles `Key(code)` by calling a pure `apply_move(board, code): BOARD` whose contracts preserve that invariant, then returns `Continue(next, LIST.empty.prepend(Print(view(next))))`, where `view(board): STRING` is pure. A `SetTimer`/`Tick` pair drives animation or a turn clock. Because the whole loop is a function of the event sequence, replays and lockstep netcode only need the event log.

### 7.3 Testing a step function

Put the transition logic in a `functions` module (for example `functions ECHO_LOGIC` with `step_logic(s, e)`) and have the reactive `step` delegate to it. A terminating `program` can then feed it a fixed list of events and print the results, and `specify` lemmas can state multi-step properties as ordinary recursion over a `LIST[EVENT]` with `variant events.length`.

## 8. Marking and tooling

- `gardelc --inspect` labels the module `reactive program` and shows the contracts of `init` and `step`, so a reviewer or AI agent can see at a glance which programs do not terminate.
- New diagnostics in the existing Phase 4 family: `"Phase 4 (Reactive Violation)"` with tags `missing_init`, `missing_step`, `main_in_reactive`, `state_type_mismatch` (`init` and `step` disagree on `S`), `io_in_reactive`, and `shutdown_must_stop`. Invariant failures stay ordinary construction-site diagnostics, with the invariant clause tag (for example `within_limit`).
- `--json-diagnostics` counterexamples for `step` include the failing `EVENT` value.

## 9. Implementation plan

1. **Parser and checker:** add `ModuleKind::ReactiveProgram`. Accept `reactive program`, require `init` and `step`, reject `main`, unify their state type, reject `IO` inside it, and add the `shutdown_must_stop` obligation.
2. **Prelude:** add `STEP`, `COMMAND`, `EVENT`, and `CONN_ID` under `stdlib/core/`, verified in `--stdlib` mode like the other prelude files.
3. **Verifier:** no new obligation kinds beyond `shutdown_must_stop`. Construction sites and termination already cover the rest.
4. **Codegen/runtime:** a host-side C loop: call `init`, then repeatedly run the returned commands, wait for the next event, and call `step`. Commands run with non-blocking OS calls (poll/epoll or the platform equivalent). The loop is not Gardel code, and no loop construct is added to the language.
5. **Command families, smallest first:** `Print` and `Shutdown`, then `SetTimer`/`Tick`, then `ReadLine`/`Line` and raw `Key` (TUI, games), then `Listen`/`Send`/`Close` (sockets, HTTP).
6. **Event log:** an optional runtime flag that records events and replays a log, for the determinism guarantee in section 5.

## 10. Partial functions (second, narrower feature)

Status: **design proposal, to be built after reactive programs.** Syntax is illustrative and untested.

`reactive` covers programs that run forever by design. It does not help when a single computation has no `variant`, as in interpreters, unbounded search, or fixpoint iteration. For those, add an explicitly marked **partial** routine. This follows Idris (`partial`), F\* (`Div` effect), and Dafny (`decreases *`), and matches Gardel's Hoare-logic base. A partial routine has **partial correctness** (`{P} C {Q}`: if it terminates, `Q` holds), while ordinary routines keep **total correctness**.

### 10.1 Syntax

```eiffel
partial eval(prog: PROGRAM, env: ENV): RESULT[EVAL_ERROR, VALUE]
  do
    ...  -- may recurse with no variant
  ensure
    well_typed: Result.is_ok implies prog.type_of = Result.get.type_of
  end
```

`partial` is a routine modifier written before the name (`RoutineDecl ::= "partial"? LowerIdent ...`) and is shown by `gardelc --inspect`. Unmarked routines remain total.

### 10.2 Rules

1. **Explicit marker.** Only routines declared `partial` may omit a `variant` on a recursive call group. Everything else is unchanged (`variant_missing` still applies).
2. **One-way firewall.** Total routines must not call partial routines. Partial routines may call total ones. A call from total to partial is rejected with `"Phase 4 (Partiality Violation)"`.
3. **Partial `main` is the only entry.** A terminating `program` may declare `partial main`. Without this, rules 2 and 6 together would make every partial routine unreachable, because nothing total may call one. A program with a partial `main` is labeled "may not terminate" by `--inspect`.
4. **Never in specifications.** `specify`, `require`, `ensure`, `invariant`, `variant`, `check`, `assume`, and `law` must not mention partial routines. In Boogie a partial routine is a `procedure` with no `function` definition and no definitional axiom. Otherwise `f(n) do f(n) end` would give the axiom `f(n) = f(n)` with any claimed postcondition, which lets the solver prove anything.
5. **Weaker, stated guarantee.** For a partial routine the verifier proves `ensure` only on return, assuming the callee contract at recursive calls without a termination check (sound for partial correctness). `--inspect` labels the contract "partial correctness".
6. **Where it may appear (first version).** Only in terminating `program` modules. Not in `reactive program` (whose `init` and `step` must stay total, so a partial routine there would be unreachable), and not in `module` or `functions`, so it cannot leak into the verified core.
7. **Evaluation strategy.** §10 promises automatic fork-join of independent `let` bindings. If a divergent call is evaluated speculatively in parallel, even when its result is never used, it could hang the program. Calls to partial routines must therefore be evaluated **only when demanded** and never forked speculatively. This constrains the BendRT/C11 backend and should be prototyped before committing to the feature.

### 10.3 Relationship to reactive programs

A reactive `step` must stay total and therefore cannot call a partial routine at all. There is no "bounded wrapper" around a partial call: a pure total language has no timeout primitive, so the only way to bound such a computation is to build the bound into it with fuel (10.4). A reactive program that evaluates user-supplied code calls `eval_fuel` with a fuel limit per event, and if the computation needs more, it carries the remaining work in its state and continues on a later `Tick`.

### 10.4 Fuel-bounded alternative (stays total)

Many uses of `partial` can be written as a total routine with explicit fuel, which needs no new feature:

```eiffel
eval_fuel(prog: PROGRAM, env: ENV, fuel: NAT): RESULT[EVAL_ERROR, VALUE]
  variant
    fuel
  do
    if fuel = 0 then
      Error(OutOfFuel)
    else
      ... eval_fuel(next_prog, next_env, fuel - 1) ...
    end
  end
```

The result type has an explicit `OutOfFuel` error, so termination is proven, the verifier keeps total correctness, and the caller chooses the bound. This is the recommended default. Reserve `partial` for programs where no sensible bound exists.

### 10.5 Example: tiny interpreter

`partial eval` above runs an arbitrary guest program, so it may loop forever on a guest `while true`. Its contract states only what holds when it returns. A command-line `program` with `partial main` may call it directly. A reactive server that receives `Received(conn, data)` decodes `data` with a boundary routine and calls `eval_fuel` instead, so a malicious input cannot stall the server.

### 10.6 Implementation plan

1. Parser: accept the `partial` modifier, including on `main`.
2. Checker: enforce rules 1, 2, 3, 4, and 6 (call-graph firewall, no use in specifications, placement).
3. Verifier: encode partial routines as Boogie procedures with partial-correctness obligations only (no termination VC, no function axiom).
4. Backend: demand-driven evaluation for partial calls (rule 7). This is the riskiest step, so prototype it first.
5. Tooling: `--inspect` marking, plus an optional hint when a partial routine has an obvious fuel rewrite.

## 11. Open questions

- Should a reactive program be allowed to declare that it never stops (embedded use), which would drop rule 4 and the `Shutdown` event?
- Command results: is "outcomes come back as events" enough, or do some commands need a correlation id (for example, several `ReadLine` requests in flight)?
- Concurrency: the first version handles one event at a time. Parallel handling of independent connections would need a proof that their state updates commute, or per-connection state.
- Resource bounds: the echo invariant already bounds connection bookkeeping. Should contracts be able to bound total state size in general, so "runs forever without unbounded memory growth" becomes a checked property?
- Liveness and fairness: deferred, and documented as intentionally not claimed.
- `LIST[COMMAND]` versus `SEQ[COMMAND]` for command batches: `LIST` is simpler and sequential, which matches execution order.
- Partial functions: can the backend guarantee demand-driven evaluation of partial calls without losing automatic parallelism elsewhere?
- Partial functions: should a dedicated partial module kind come later, and what may import it?

## 12. Required spec changes (v0.4 to v0.5)

| Spec section | Change |
| --- | --- |
| §2 principle 2 | "Every routine terminates" becomes "every routine terminates, except `partial` routines (§10 of this proposal). Non-termination of a whole program is possible only in a `reactive program`, through the runtime loop." |
| §3 table | Define `EXIT_CODE` (used by §11 and `gardelc` as a `U64` alias but never declared in the spec). |
| §4 | Add `reactive program` as a fourth unit kind. It has `init` and `step`, uses no `IO`, and follows the one-module-per-file rule (§4.2). |
| §4.1 inventory, §15 prose | Add `STEP`, `COMMAND`, `EVENT`, `CONN_ID` to the prelude list. |
| §11.1 | "Only a `program` module may use IO" stays true. Add that a `reactive program` uses commands, not `IO`. |
| §12 obligations table | Add the `shutdown_must_stop` row. For partial routines, the termination row does not apply and the routine row becomes partial correctness. |
| §15 grammar | `ModuleUnit` gains `ReactiveProgramDecl ::= "reactive" "program" UpperIdent FeatureSection* "end"`. `RoutineDecl` gains an optional leading `"partial"`. |
