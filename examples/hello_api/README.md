# Hello API Example (`hello_api`)

A small, formally verified HTTP/1.1 service written in Gardel on top of the [`ghttp`](../../ghttp) package.

The server listens on `127.0.0.1:8090` (or `GARDEL_LISTEN_ADDR`), accepts up to a proven limit of 64 concurrent TCP connections, incrementally parses incoming HTTP/1.1 requests, and dispatches them through an extensible `HTTP_ROUTER`:

- `GET /` $\rightarrow$ `200 OK` with body `"OK"` (inherited from `ghttp`'s default server router, endpoint ID `1`)
- `GET /hello` $\rightarrow$ `200 OK` with body `"hello from a package"` (registered by `HELLO_API`, endpoint ID `2`)
- Unregistered paths $\rightarrow$ `404 Not Found`
- Disallowed methods on registered paths $\rightarrow$ `405 Method Not Allowed`

---

## Project Structure & Package Dependency

- [`Gardel.toml`](Gardel.toml): Declares `name = "hello-api"` and a local path dependency on `ghttp = { path = "../../ghttp" }`.
- [`src/hello_api.gdl`](src/hello_api.gdl): Defines `reactive program HELLO_API`:
  - `routes`: Extends `HTTP_ROUTER.default_server_router` with `HTTP_ROUTE.make(Get, "/hello", 2)` and proves `Result.num_routes = 2`.
  - `endpoint`: Handles endpoint ID `2` (`"hello from a package"`) and delegates other IDs to `HTTP_APPLICATION.default_endpoint_handler`.
  - `answer` & `on_bytes`: Feeds incoming TCP chunks into `HTTP_PARSER.feed`, dispatches completed requests via `HTTP_APPLICATION.dispatch_with`, serializes responses with `HTTP_SERIALIZER.serialize`, and closes the connection after sending.
  - `init` & `step`: Initializes `HTTP_SERVER_STATE` with capacity `64` on port `8090` and handles `Accepted`, `Received`, `SendFailed`, `Closed`, `ListenFailed`, and `Shutdown`.

Because `ghttp` is imported as a dependency in `Gardel.toml`, only `ghttp`'s public contracts are visible to `HELLO_API` during modular verification.

---

## Verification & Compilation

From the repository root, verify and compile `hello_api` using `gardelc`:

```powershell
# Using the project manifest (--project <DIR>)
./gardelc/target/release/gardelc.exe --project examples/hello_api --strict

# Or directly passing the directory operand
./gardelc/target/release/gardelc.exe examples/hello_api --strict
```

If `gardelc` is on your `PATH`, you can also run it from inside `examples/hello_api`:

```powershell
cd examples/hello_api
gardelc --strict
```

This verifies all contracts and produces the native executable:
- From the repo root: `./build/hello_api_bin.exe` (or `examples/hello_api/build/hello_api_bin.exe` when built inside the project directory).

---

## Running the Live Server

Start the server using `gardelc --run` or by invoking the compiled binary directly:

```powershell
# Build, verify, and run immediately
./gardelc/target/release/gardelc.exe --project examples/hello_api --run

# Or run the compiled executable directly
./build/hello_api_bin.exe
```

By default, `Listen(8090)` binds to `127.0.0.1:8090` (override the bind IP with `GARDEL_LISTEN_ADDR`, e.g., `GARDEL_LISTEN_ADDR=0.0.0.0`).
The server runs silently while waiting for connections. Press `Ctrl+C` to send a `Shutdown` event and exit cleanly with code `0`.

> **Troubleshooting `hello_api: listen failed`:**
> Because the server runs silently without a startup banner, an earlier instance of `hello_api_bin.exe` may already be running and holding port `8090` (`WSAEADDRINUSE` / OS error `10048`). Check or stop the existing process with:
> ```powershell
> Get-NetTCPConnection -LocalPort 8090
> Stop-Process -Name hello_api_bin
> ```

---

## Testing Live HTTP Requests

While the server is running in one terminal, open a second terminal and query the endpoints:

### Using `curl`

```powershell
# Custom route registered by HELLO_API (returns "hello from a package")
curl.exe -i http://127.0.0.1:8090/hello

# Default root route from ghttp (returns "OK")
curl.exe -i http://127.0.0.1:8090/

# Unregistered route (returns 404 Not Found)
curl.exe -i http://127.0.0.1:8090/missing
```

### Using PowerShell (`Invoke-WebRequest`)

```powershell
Invoke-WebRequest -Uri "http://127.0.0.1:8090/hello" | Select-Object StatusCode, Content
```

---

## Deterministic Scripted Execution & Replay (`GARDEL_EVENTS`, `GARDEL_RECORD`, `GARDEL_REPLAY`)

Because `HELLO_API` is a reactive program (§11.6), you can test HTTP parsing, routing, and wire serialization deterministically without opening network sockets.

### 1. In-Memory Event Script (`GARDEL_EVENTS`)

**PowerShell:**
```powershell
$env:GARDEL_EVENTS = "Accepted:1 Received:1:GET%20/hello%20HTTP/1.1%0D%0A%0D%0A Accepted:2 Received:2:GET%20/%20HTTP/1.1%0D%0A%0D%0A Shutdown"
./build/hello_api_bin.exe
Remove-Item Env:\GARDEL_EVENTS
```

**Bash:**
```bash
GARDEL_EVENTS="Accepted:1 Received:1:GET%20/hello%20HTTP/1.1%0D%0A%0D%0A Accepted:2 Received:2:GET%20/%20HTTP/1.1%0D%0A%0D%0A Shutdown" ./build/hello_api_bin.exe
```

**Output (on `stderr`):**
```text
net: Listen 8090
net: Send 1 HTTP%2F1%2E1%20200%0D%0AContent%2DLength%3A%2020%0D%0A%0D%0Ahello%20from%20a%20package
net: Close 1
net: Send 2 HTTP%2F1%2E1%20200%0D%0AContent%2DLength%3A%202%0D%0A%0D%0AOK
net: Close 2
```

### 2. Recording & Replaying a Live Session (`GARDEL_RECORD` / `GARDEL_REPLAY`)

```powershell
# Record a live session to a log file
$env:GARDEL_RECORD = "hello_api.log"
./build/hello_api_bin.exe
# (Make HTTP requests from another terminal, then press Ctrl+C)
Remove-Item Env:\GARDEL_RECORD

# Replay the recorded session offline and verify identical commands/exit code
$env:GARDEL_REPLAY = "hello_api.log"
./build/hello_api_bin.exe
Remove-Item Env:\GARDEL_REPLAY
```

