# Echo Server Example

A statically verified concurrent TCP echo server written in Gardel.

The server listens on `127.0.0.1:8888` (or `GARDEL_LISTEN_ADDR`), accepts incoming connections up to a proven limit of 64 concurrent clients, and echoes back any received byte stream.

---

## Architecture & Verification Guarantees

This project demonstrates Gardel's **Reactive Program** architecture (§11.6 of the specification):

- [`src/echo_server.gdl`](src/echo_server.gdl): Defines the `reactive program ECHO_SERVER` with total `init` and `step` routines handling event transitions (`Listen`, `Accepted`, `Received`, `Closed`, `ListenFailed`, `Shutdown`).
- [`src/server_state.gdl`](src/server_state.gdl): Defines an opaque state module `module SERVER_STATE` (`representation {NONE}`).
  - **Proven Invariant:** `invariant within_limit: live.length <= limit`
  - The compiler and SMT solver prove that the server never exceeds its connection capacity of 64 clients after `init` and after every `step` of an unbounded run.
- [`Gardel.toml`](Gardel.toml): Declares the project package identity (`name = "echo-server"`, `version = "0.1.0"`).

### Why `CONN_ID` Is Opaque & Why the Server Runs Silently

- **Opaque connection handles (`CONN_ID`):** In Gardel's Standard Prelude, `CONN_ID` has `representation {NONE}` and no creator. User code can compare handles (`=`, `!=`, `equals`, `less_than`) and store them in `SERVER_STATE`, but can never inspect or forge a raw socket/connection number.
- **Silent daemon operation:** Following Unix daemon convention, `ECHO_SERVER` emits no `Print` commands during normal operation (it only prints `"cannot listen on port 8888"` if `ListenFailed` occurs). To inspect connection IDs, events, and commands, use the runtime's built-in event recorder (`GARDEL_RECORD`) or scripted runner (`GARDEL_EVENTS`) described below.

---

## Verification & Compilation

You can verify and compile the project using either manifest mode (`--project`) or a direct directory operand:

```bash
# Using the manifest (--project <DIR>)
gardelc --project examples/echo_server --strict

# Or from inside the directory (bare gardelc detects Gardel.toml)
cd examples/echo_server
gardelc --strict

# Or without using the manifest (direct directory operand)
gardelc examples/echo_server --strict
```

A successful run outputs:
```text
[0/5] Standard Prelude injected (§4.1): 15 modules ["OPTION", "SEQ", "BIGNAT", "STRING_ERROR", "RESULT", "BYTES", "STRING", "IO_ERROR", "LIST_OPS", "FUNCTION_OPS", "CONN_ID", "COMMAND", "KEY", "EVENT", "STEP"], 3 concepts (trusted, verified in --stdlib bootstrap mode)
[1/5] Linked project of 2 files into one flat namespace (§4.2): 2 modules ["ECHO_SERVER", "SERVER_STATE"], 3 concepts ["EQ", "CANONICAL_EQ", "ORD"], fuel: 2
      examples/echo_server/src/echo_server.gdl: group None, ["ReactiveProgram ECHO_SERVER"]
      examples/echo_server/src/server_state.gdl: group None, ["Module SERVER_STATE"]
[2/5] Checked §4 Visibility, §9 Concept Laws, §11 IO Boundary & §12 Direct Verification:
      0 errors, 0 warnings — all visibility, concept laws, IO linearity, overflow, and contract obligations satisfied.
[3/5] Emitted direct Boogie VCs & fuel axioms -> build/echo_server.bpl, build/server_state.bpl (one per module; primary `ECHO_SERVER`)
[4/5] Emitted monomorphized Bend v2 target    -> build/echo_server.bend
[5/5] Emitted C11 runtime target              -> build/echo_server.c
      Compiled native executable              -> build/echo_server_bin.exe
```

---

## Running the Live Server

Start the server via `gardelc --run` or by running the compiled executable directly:

```bash
gardelc --project examples/echo_server --run
# or directly:
./build/echo_server_bin.exe
```

By default, `Listen(8888)` binds to loopback (`127.0.0.1:8888`). To bind to another address, set `GARDEL_LISTEN_ADDR` (for example `GARDEL_LISTEN_ADDR=0.0.0.0`).

Because the server does not print a startup banner, the terminal will wait quietly while listening on port `8888`. Press `Ctrl+C` at any time to deliver a `Shutdown` event and stop the server cleanly with exit code `0`.

---

## Testing Live Connections

While the server is running in one terminal, open a second terminal to send data.

### Using PowerShell

```powershell
$client = New-Object System.Net.Sockets.TcpClient("127.0.0.1", 8888)
$stream = $client.GetStream()
$writer = New-Object System.IO.StreamWriter($stream)
$reader = New-Object System.IO.StreamReader($stream)

$writer.WriteLine("Hello from PowerShell!")
$writer.Flush()
$reader.ReadLine()   # Returns: Hello from PowerShell!

$client.Close()
```

### Using `nc` (Netcat)

```bash
nc 127.0.0.1 8888
Hello Gardel!
# Echoed response: Hello Gardel!
```

---

## Inspecting Connection IDs & Events (`GARDEL_RECORD` & `GARDEL_EVENTS`)

Because a reactive program is a pure function of its arguments and event stream, the Gardel runtime provides environment variables to inspect, script, record, and deterministically replay every connection ID, event, and command (§11.6).

| Environment Variable | Effect |
| :--- | :--- |
| `GARDEL_EVENTS=<script>` | Run deterministically from an in-memory event script without opening real sockets; prints all `net:` commands and connection IDs to `stderr`. |
| `GARDEL_RECORD=<file>` | Run the live server and record every connection ID, event, command, and exit code to `<file>`. |
| `GARDEL_REPLAY=<file>` | Replay a recorded log file deterministically without sockets, verifying that the program produces the exact same commands and exit code. |
| `GARDEL_LISTEN_ADDR=<ip>` | Bind `Listen(8888)` to `<ip>` instead of `127.0.0.1`. |

### 1. Scripted Inspection (`GARDEL_EVENTS`)

You can test connection admission, echoing, and the 64-connection limit without opening sockets by passing a space-separated event script in `GARDEL_EVENTS` (bytes are URL-percent-encoded when non-ASCII or containing spaces):

**PowerShell:**
```powershell
$env:GARDEL_EVENTS = "Accepted:1 Accepted:2 Received:1:hi%20there Closed:1 Received:2:%FF%00 Closed:2 Shutdown"
./build/echo_server_bin.exe
Remove-Item Env:\GARDEL_EVENTS
```

**Bash:**
```bash
GARDEL_EVENTS="Accepted:1 Accepted:2 Received:1:hi%20there Closed:1 Received:2:%FF%00 Closed:2 Shutdown" ./build/echo_server_bin.exe
```

**Output (on `stderr`):**
```text
net: Listen 8888
net: Send 1 hi%20there
net: Send 2 %FF%00
```

If a 65th client connects while 64 connections (`1..64`) are active, the runtime trace shows `net: Close 65` immediately.

### 2. Recording a Live Session (`GARDEL_RECORD`)

To see the exact connection IDs and payloads of real TCP clients connecting to the server, run the binary with `GARDEL_RECORD`:

**PowerShell:**
```powershell
$env:GARDEL_RECORD = "echo_session.log"
./build/echo_server_bin.exe
# Connect with clients in another terminal, then press Ctrl+C to stop the server
Remove-Item Env:\GARDEL_RECORD
Get-Content echo_session.log
```

**Bash:**
```bash
GARDEL_RECORD=echo_session.log ./build/echo_server_bin.exe
# Connect with clients in another terminal, then press Ctrl+C
cat echo_session.log
```

The resulting `echo_session.log` records the arguments, the `Listen 8888` command, every `Accepted:<id>`, `Received:<id>:<bytes>`, `Send <id> <bytes>`, `Closed:<id>`, and the final `Shutdown` and `exit 0`.

### 3. Deterministic Replay (`GARDEL_REPLAY`)

Any log recorded with `GARDEL_RECORD` can be replayed offline as a regression test:

**PowerShell:**
```powershell
$env:GARDEL_REPLAY = "echo_session.log"
./build/echo_server_bin.exe
Remove-Item Env:\GARDEL_REPLAY
```

**Bash:**
```bash
GARDEL_REPLAY=echo_session.log ./build/echo_server_bin.exe
```

During replay, no sockets are opened: the runtime feeds the recorded events into `init` and `step` and verifies that every emitted command and the exit status match the recording (exiting with status `3` on the first mismatch).
