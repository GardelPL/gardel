# Gardel HTTP (`ghttp`)

A formally verified HTTP/1.1 protocol library and reactive TCP server written in Gardel.

`ghttp` separates the **pure HTTP protocol core** (domain model, incremental parser, wire serializer, declarative router, and pure handlers) from the **reactive TCP shell** (`HTTP_SERVER`), allowing the compiler and SMT solver (Boogie/Z3) to statically verify protocol invariants, framing guarantees, and resource bounds without opening a network socket.

---

## Architecture: Pure Core + Reactive Shell

```text
+-------------------------------------------------------------------+
| HTTP Application Layer (src/application/)                         |
|   HTTP_APPLICATION: Pure handlers (HTTP_REQUEST -> HTTP_RESPONSE) |
+-------------------------------------------------------------------+
| Pure HTTP Core (src/core/, src/parser/, src/serializer/, routing/)|
|   Domain Model : HTTP_METHOD, HTTP_VERSION, HTTP_STATUS,          |
|                  HTTP_ERROR, HTTP_HEADER, HTTP_HEADERS,           |
|                  HTTP_REQUEST, HTTP_RESPONSE                      |
|   Parser       : HTTP_PARSER_STATE, HTTP_PARSE_RESULT,            |
|                  HTTP_PARSER (incremental feed & framing checks)  |
|   Router       : ROUTE_RESULT, HTTP_ROUTE, HTTP_ROUTER            |
|   Serializer   : HTTP_SERIALIZER (verified CRLF wire framing)     |
+-------------------------------------------------------------------+
| Reactive TCP Shell (src/server/)                                  |
|   CONNECTION_STATUS, HTTP_CONNECTION, HTTP_SERVER_STATE,          |
|   reactive program HTTP_SERVER (Listen, Accepted, Received,       |
|   Send, Close, SendFailed, ListenFailed, Shutdown)                |
+-------------------------------------------------------------------+
```

---

## Verified Obligations (SMT Proofs vs. Executable Tests)

### 1. Static SMT Proofs (`gardelc --project ghttp --strict`)

| Category | Module(s) | Proven Properties |
| :--- | :--- | :--- |
| **Domain Invariants** | `HTTP_METHOD`, `HTTP_VERSION`, `HTTP_STATUS`, `HTTP_ERROR` | `CANONICAL_EQ` & `EQ` laws (`reflexive`, `symmetric`, `transitive`, `agrees_with_structural`); status code range $100 \le \text{code} \le 599$; exhaustive 5-class partition (`1xx`..`5xx`); `safe_methods_are_idempotent`; `safe_methods_disallow_body`. |
| **Header & Message Framing** | `HTTP_HEADER`, `HTTP_HEADERS`, `HTTP_REQUEST`, `HTTP_RESPONSE` | Non-empty header names ($\le 128$ bytes), bounded header values ($\le 1024$ bytes), exact wire line length (`name_len + value_len + 4`), bounded header collection ($\le 64$ headers), safe methods carry empty bodies, RFC 9112 §6.3 body-forbidden status codes (`1xx`, `204`, `304`) carry $0$ body bytes. Header lookup ignores the case of names (`found_after_add`: a field just added is found by any spelling). A response's `Content-Length` is computed from its body and reads back as the body's length. |
| **Incremental Parser** | `HTTP_PARSER_STATE`, `HTTP_PARSE_RESULT`, `HTTP_PARSER`, `HTTP_HEAD_PARSER`, `HTTP_FRAMING` | The state keeps every byte received, within `max_request_line + max_header_bytes + max_body_bytes` ($\le 75{,}776$ bytes), and each `feed` parses all of them again, so where TCP cut them does not matter. `HTTP_FRAMING.faithful` holds of every result: a `NeedMore` state holds every byte received, in order, under the same limits; after `Complete(req, rest)`, `rest` is the tail of those bytes and `req.body` the `Content-Length` bytes just before it. Exhaustive trichotomy of `NeedMore`, `Complete` and `Failed`; safe indexed byte access with no out-of-bounds read or overflow. |
| **Wire Serialization** | `HTTP_SERIALIZER` | Wire output is always non-empty ($\ge 22$ bytes), strictly larger than body length when a body is present, and bounded ($\le 65{,}567$ bytes) without arithmetic overflow. |
| **Routing** | `ROUTE_RESULT`, `HTTP_ROUTE`, `HTTP_ROUTER` | `CANONICAL_EQ` laws; exhaustive disjoint partition of `Matched`, `MethodNotAllowed`, and `NotFound`; bounded route table capacity ($\le 64$ routes). |
| **Reactive Server Safety** | `CONNECTION_STATUS`, `HTTP_CONNECTION`, `HTTP_SERVER_STATE`, `HTTP_SERVER` | Active connection count never exceeds `max_connections` ($\le 64$ in `HTTP_SERVER`, $\le 1024$ max); closing/closed connections never accept further bytes; IO linearity and totality across all runtime events. |

### 2. Executable Protocol & Integration Tests (`ghttp/tests/`)

`ghttp/tests` is a package that depends on `ghttp`, so it sees only its contracts. `GHTTP_TEST` runs the suites, then reads the parser cases of `ghttp/tests/cases.txt` on standard input. Each line is a request and what the parser must say of it. `GHTTP_TEXT` turns a result into text so two results can be compared, and checks the request whole and cut into two reads at every byte. The cases are kept out of the source because string literals are costly to verify in a module that uses `ghttp`.

| Suite | Verified Behavior |
| :--- | :--- |
| `check_domain_model` | Method safety/idempotence, status code classes (`200`, `204`), header wire length, header lookup under another case, and a computed `Content-Length`. |
| `cases.txt` (`run_cases`) | Each request is parsed whole and cut into two reads at every byte offset, and every cut must give what the whole gives: targets as received, header counts, `Content-Length` bodies, the bytes after a request (`rest`), `Content-Type` found under any case of its name, waiting for more, and each refusal (no version, bad target, unknown or lower-case method, HTTP/2, a bad header, chunked encoding (`501`), both framings, two different lengths, a bad or too large length, and a body on `GET`). |
| `check_router_and_rest_extension` | Verifies that the default server router only serves `GET /` (`200`) and rejects unregistered routes (`404`) or disallowed methods (`405`), and demonstrates building a custom REST API extension (`GET /health`, `GET /users`, `POST /users`, `DELETE /users/:id`) using `HTTP_ROUTER.register` and `HTTP_APPLICATION.dispatch_with`. |

---

## Building, Verifying & Running

### 1. Verify and Build the HTTP Server (`HTTP_SERVER`)

```powershell
./gardelc/target/release/gardelc.exe --project ghttp --strict
```

### 2. Verify and Run the Protocol Test Suite (`GHTTP_TEST`)

```powershell
./gardelc/target/release/gardelc.exe --project ghttp/tests --strict
Get-Content ghttp/tests/cases.txt | ./build/ghttp_test_bin.exe
```

### 3. Deterministic Scripted Server Execution (`GARDEL_EVENTS`)

Test connection admission, fragmented TCP delivery (`NeedMore` followed by completion), and HTTP response framing without opening sockets:

```powershell
$env:GARDEL_EVENTS = "Accepted:1 Received:1:GET%20/health%20HTTP/1.1%0D%0A%0D%0A Accepted:2 Received:2:GET%20/he Received:2:alth%20HTTP/1.1%0D%0A%0D%0A Shutdown"
# Default port (8080):
./build/http_server_bin.exe
# Custom port (e.g., 9090):
./build/http_server_bin.exe 9090
Remove-Item Env:\GARDEL_EVENTS
```

