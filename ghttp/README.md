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
|                  HTTP_REQUEST, HTTP_RESPONSE, HTTP_CONTENT_TYPE   |
|   Parser       : HTTP_PARSER_STATE, HTTP_PARSE_RESULT,            |
|                  HTTP_PARSER (incremental feed & framing checks)  |
|   Router       : ROUTE_RESULT, HTTP_ROUTE, HTTP_ROUTER            |
|   Serializer   : HTTP_SERIALIZER (verified CRLF wire framing)     |
+-------------------------------------------------------------------+
| Client Core (src/client/)                                         |
|   HTTP_REQUEST_WRITER : a request to bytes, if it reads back      |
|   HTTP_RESPONSE_READER, HTTP_RESPONSE_PARSER,                     |
|   HTTP_RESPONSE_PARSE_RESULT, HTTP_CLIENT_RESPONSE                |
|   HTTP_CLIENT_CONNECTION, HTTP_CLIENT, HTTP_EXCHANGE : one        |
|     request at a time on a connection, reused when both keep it   |
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
| **Wire Serialization** | `HTTP_SERIALIZER`, `HTTP_CONTENT_TYPE` | The head is exactly the status line, the `Content-Type` line if the response states one, and `Content-Length`, at least 35 and at most 85 bytes, or 104 with `Connection: close` (`answer_head`). Wire output is strictly larger than the body and bounded ($\le 65{,}621$ bytes) without arithmetic overflow. A media type comes from a fixed set (`Unstated`, `PlainText`, `Json`), so its line is known and holds no line break. The status line carries the response's own code for any code in 100..599. |
| **Client** | `HTTP_REQUEST_WRITER`, `HTTP_RESPONSE_READER`, `HTTP_RESPONSE_PARSER`, `HTTP_CLIENT_RESPONSE` | The writer writes only a request that `sendable` accepts: an origin-form target of printable ASCII, token field names, field values with no control byte and no space or tab at either end, and no `Content-Length` or `Transfer-Encoding` of its own. It adds `Content-Length` to a `POST`, a `PUT` and any request with a body, and its output is bounded. The response reader keeps every byte it is given, as the request parser does, so where TCP cut them does not matter (`faithful`): a reader that waits holds them all, in order, under the same limits, and a complete response is followed by the tail of them, with its body just before. `finish`, for a closed connection, never waits. A response to `HEAD`, and a 1xx, 204 or 304, has no body, which the type keeps (`no_body_when_forbidden`). The round trip, in `HTTP_ROUND_TRIP`, in two stages: the request line the writer writes parses back to the same method, target and version (`request_line_reads_back`), and a head that declares the body's length gives back the same body with nothing after it (`body_reads_back`). The parser compares a method or version with its own `to_string`, so the writer and the parser share one spelling. |
| **Routing** | `ROUTE_RESULT`, `HTTP_ROUTE`, `HTTP_ROUTER` | `CANONICAL_EQ` laws; exhaustive disjoint partition of `Matched`, `MethodNotAllowed`, and `NotFound`; bounded route table capacity ($\le 64$ routes). |
| **Reactive Server Safety** | `CONNECTION_STATUS`, `HTTP_CONNECTION`, `HTTP_SERVER_STATE`, `HTTP_SERVER` | Active connection count never exceeds `max_connections` ($\le 64$ in `HTTP_SERVER`, $\le 1024$ max); closing/closed connections never accept further bytes; IO linearity and totality across all runtime events. `HTTP_SERVER` answers one request per connection and says `Connection: close`. |
| **Persistent Connections** | `HTTP_PERSISTENCE`, `HTTP_PIPELINE`, `HTTP_BATCH`, `HTTP_BATCH_END` | `HTTP_PERSISTENCE.request_keeps_open`: an HTTP/1.1 request keeps its connection open unless a `Connection` field lists `close`, in any case and among other options; an HTTP/1.0 request closes it (`response_keeps_open` is the same for a client). `HTTP_PIPELINE.split` reads the requests that bytes complete on a connection, one after another from what the last one left, and ends with `ReadOn`, `CloseAfter` or `FailAfter`. Proven: no connection answers more than `HTTP_CONNECTION.max_requests` (100) requests (`capped`), a connection read on has counted every request of the batch (`counted`), and a batch that fails leaves room for its error's answer. `HTTP_SERIALIZER.serialize_answer` adds `Connection: close` when the server closes after the answer, and sends no body for `HEAD`. |
| **Client Connections** | `HTTP_CLIENT_CONNECTION`, `HTTP_CLIENT`, `HTTP_EXCHANGE` | A client sends one request at a time on a connection: `sending` takes a free connection and gives one that waits, and only an answer frees it. `HTTP_CLIENT.receive` gives `Waiting`, `Reuse`, `Done` or `Broken`, and `closed` gives `Done`, `Broken` or `Unanswered`, the last when no byte of the answer came, so that an idempotent request may be sent again on a new connection. Proven: a connection that waits holds every byte of the answer, in order; a connection is reused only when the request and the answer both keep it open (`HTTP_PERSISTENCE`), and then counts one more answer, below `max_exchanges` (100) by its invariant; a close never leaves a connection waiting or reused. An answer whose body runs to the close is never reused, and neither is one followed by more bytes. |
| **Idle Timeout** | `HTTP_CONNECTION`, `HTTP_SERVER_STATE`, `HTTP_SWEEP` | A connection counts the sweeps since it was accepted or last answered, below `HTTP_CONNECTION.max_idle_ticks` (5) by its invariant. `HTTP_SERVER_STATE.sweep` closes each connection the sweep would take to 5 and keeps the others one sweep older: kept plus closed is the number open, and every kept connection has waited through a sweep (`HTTP_SWEEP.all_waited`). The server sets the timer: `grest` sweeps every second. |

### 2. Executable Protocol & Integration Tests (`ghttp/tests/`)

`ghttp/tests` is a package that depends on `ghttp`, so it sees only its contracts. `GHTTP_TEST` runs the suites, then reads the parser cases of `ghttp/tests/cases.txt` on standard input. Each line is a request and what the parser must say of it. `GHTTP_TEXT` turns a result into text so two results can be compared, and checks the request whole and cut into two reads at every byte. The cases are kept out of the source because string literals are costly to verify in a module that uses `ghttp`.

| Suite | Verified Behavior |
| :--- | :--- |
| `check_domain_model` | Method safety/idempotence, status code classes (`200`, `204`), header wire length, header lookup under another case, and a computed `Content-Length`. |
| `cases.txt` (`run_cases`) | Each request is parsed whole and cut into two reads at every byte offset, and every cut must give what the whole gives: targets as received, header counts, `Content-Length` bodies, the bytes after a request (`rest`), `Content-Type` found under any case of its name, waiting for more, and each refusal (no version, bad target, unknown or lower-case method, HTTP/2, a bad header, chunked encoding (`501`), both framings, two different lengths, a bad or too large length, and a body on `GET`). |
| `cases.txt`, `write` lines and every accepted request | The writer: each request the parser accepts, without its framing fields, is written and parsed back to the same method, target, version, fields in order, and body, with nothing after it; `write` lines also give the exact bytes. The request line and the body are also proven (`HTTP_ROUND_TRIP`); that the parser finds the writer's line ends, and reads its field lines back in order, is checked here only. |
| `cases.txt`, `response` and `closed` lines | The response reader, whole and cut into two reads at every byte: bodies by `Content-Length`, the answer to `HEAD`, 1xx, 204 and 304 with no body, a body that runs to the close, a status line with no reason phrase, the bytes after a response, waiting for more, a response cut short by the close (`IncompleteResponse`), and each refusal (HTTP/2, a lower-case version, codes outside 100..599, a bad status line, chunked encoding, both framings, a bad or repeated length, a bad field). |
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

