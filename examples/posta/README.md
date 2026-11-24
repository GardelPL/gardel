# Posta — Verified HTTP / REST API Workbench (`posta`)

A full-screen TUI HTTP/REST client (Postman / Insomnia for the terminal) written in Gardel, uniting [`gtui`](../../gtui) `0.6.0`, [`ghttp`](../../ghttp), and [`gjson`](../../gjson) inside a single pure, deterministic reactive state machine.

Every protocol, JSON, and concurrency safety obligation is proved statically at compile time by Boogie/Z3 under `--strict` with zero `assume` and zero runtime assertion overhead:

* **Bounded In-Flight Requests (`active_count <= 1`):** `IN_FLIGHT` statically proves at most one request is active at a time (`require currently_idle: not Current.is_active`), preventing duplicate socket launches or UI hangs.
* **Zero Socket & Timer Leaks (`idle_clean`):** Every transition back to Idle (`on_received`, `on_closed`, `on_connect_failed`, `on_tick` timeout, and `cancel`) statically proves `link.is_none`, `client.is_none`, and emits `Close(conn)` / `CancelTimer(id)`.
* **Connection Isolation & RFC 9110/9112 Framing:** Incoming bytes only feed `ghttp::HTTP_CLIENT.receive` when `Current.is_on(conn)` holds; stray sockets are immediately closed. Requests are serialized only after passing `HTTP_REQUEST_WRITER.sendable(req)` and safe-method body invariants.
* **Live RFC 8259 JSON Validation:** The multi-line `TEXT_AREA` request body editor runs `gjson::JSON.parse_text` live, displaying `✓ Valid JSON (gjson)` or the exact byte offset of any syntax error.

```text
 File(Alt+F)  Request(Alt+R)  Theme(Alt+T)  Help
 Posta › Create Todo › POST                         ✓ In-Flight: 0/1 [PROVEN]
┌ Collections ─────────┐┌ HTTP/1.1 Target ───────────────────────────── ghttp ┐
│ ▾ Todo API (:8091)   ││ [POST ▾] http://127.0.0.1:8091/todos      [▶ Send ] │
│   GET  /todos        │├ Body(b) | Headers(h) | Contracts(c) ────────────────┤
│   POST /todos   ◄    ││  1 │ {"title":"Verify Posta","done":false}          │
│   GET  /todos/1      ││                                                     │
│   DEL  /todos/1      ││ ✓ Valid JSON (gjson)                                │
│ ▾ API Gateway (:8092)├┼ Response(1) | Resp Headers(2) ──────────────────────┤
│   GET  /health       ││ HTTP 201 · 4ms · 46 B  ·  ✓ Valid JSON (gjson)      │
│ ── Latency (ms) ──── ││ {"id":1,"title":"Verify Posta","done":false}        │
│ ▁▂▁▃▂                ││                                                     │
└──────────────────────┘└─────────────────────────────────────────────────────┘
 Posta · Idle   F5/Ctrl+S:Send  Esc:Cancel  Tab:Focus  m:Method  u:URL  q:Quit
```

## Verify, Build, and Run

From the repository root (ensure `boogie` and `Z3_EXE` are configured):

```powershell
gardelc --project examples/posta --strict
./build/posta_bin.exe                    # defaults to 127.0.0.1:8091
./build/posta_bin.exe 9000               # applies port 9000 to all endpoints
./build/posta_bin.exe 127.0.0.1:9000     # applies host:port to all endpoints
```

### Live Full-Stack Verified Demo (Client ↔ Server)

Run [`todo_api`](../todo_api) in one terminal and `posta_bin` in another so **both** the HTTP client and HTTP server are 100% verified Gardel binaries talking over real TCP:

```powershell
# Terminal 1
gardelc --project examples/todo_api --strict
./build/todo_api_bin.exe 8091

# Terminal 2
./build/posta_bin.exe
# Select `POST /todos` or `GET /todos` under `Todo API (:8091)` and press F5 / Ctrl+S!
```

Inspect the Eiffel-style verified contract interfaces directly:

```powershell
gardelc --project examples/posta --inspect IN_FLIGHT --no-verify
gardelc --project examples/posta --inspect ENDPOINT --no-verify
```

## 30-Second Sabotage Demo (Socket Leak / Invariant Violation)

To see Boogie/Z3 catch a resource-hygiene bug at compile time:

1. Open [`src/domain/in_flight.gdl`](src/domain/in_flight.gdl) and find `cancel`.
2. Forget to clear the active state when cancelling by replacing `IN_FLIGHT.idle(Current.req_tag)` with `Current`:
   ```eiffel
   FLIGHT_STEP.make(Current, cmds, Some(resp))
   ```
3. Run `gardelc --project examples/posta --strict`.
4. **Result:** Boogie/Z3 refuses to compile `posta` and pinpoints the violated postconditions (`now_idle` / `no_conn`).

This negative proof is automated in [`gardelc/tests/posta.rs`](../../gardelc/tests/posta.rs).

## Controls

### Keyboard

| Key | Action |
| :--- | :--- |
| `F5` / `Ctrl+S` | Send HTTP/1.1 request (or `Enter` in URL bar / Collections tree) |
| `Esc` | Cancel in-flight request (or return focus to Collections) |
| `Tab` | Cycle focus: Collections → URL Bar → Request Body → Response |
| `p` | Open **Set Base Host:Port** modal (updates all sidebar endpoints & URL bar) |
| `a` | Save current URL, Method, and Body as a new endpoint under **Custom** |
| `m` | Cycle HTTP Method (`GET` → `POST` → `PUT` → `DELETE`) |
| `u` | Jump focus directly to URL Bar |
| `b` / `h` / `c` | Switch Request tabs: **Body** (`TEXT_AREA`) / **Headers** / **Contracts** |
| `1` / `2` | Switch Response tabs: **Response Body** / **Resp Headers** |
| `t` | Cycle Theme (`Dark`, `Light`, `Nord`, `Dracula`) |
| `F10` | Open top menu bar |
| `?` | Toggle Help popup |
| `q` | Quit |

### Mouse

* Click folders/endpoints in the **Collections** `TREE_VIEW` to expand/collapse and load presets.
* Click `[POST ▾]` to cycle method, click the **URL bar** to edit, or click `[▶ Send]` / `[■ Cancel]`.
* Click and edit directly inside the **Body** `TEXT_AREA` (supports cursor placement, drag selection, scroll wheel, undo/redo).
* Click **Tabs** (`Body`, `Headers`, `Contracts`, `Response`, `Resp Headers`) and top **Menu** items.

## Tests

```powershell
cargo test --manifest-path gardelc/Cargo.toml --test posta
```
