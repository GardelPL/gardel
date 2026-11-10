# Hello API Example (`hello_api`)

A small, verified REST service written in Gardel on the [`grest`](../../grest) package. It is its
handlers, its routes and two lines: [`REST_SERVER`](../../grest/src/rest/rest_server.gdl) holds the
connections, and proves that each request is answered once and its connection closed after the
answer, with at most 64 connections open.

| Request | Answer |
| :--- | :--- |
| `GET /` | `200`, `OK` as text |
| `GET /hello` | `200`, `hello from a package` as text |
| `GET /hello/:name` | `200`, `{"hello":"<name>"}`, the name decoded (`/hello/ana%20b` gives `ana b`) |
| `POST /echo` | `200`, the body as text, or `400` if it is not UTF-8 |
| Another method on one of these paths | `405`, `{"error":"method not allowed"}` |
| Any other path | `404`, `{"error":"not found"}` |
| A target that cannot be read, such as `/x%` | `400`, `{"error":"<reason>"}` |

A `HEAD` request is answered by the `GET` route. Bytes that cannot be a request get `ghttp`'s
status for the error, with no body.

## Project

- [`Gardel.toml`](Gardel.toml) depends on `grest`, and on `ghttp` and `gjson` for the types of
  its requests, responses and JSON values, as a package sees only the packages it names.
- [`src/hello_api.gdl`](src/hello_api.gdl) is `reactive program HELLO_API`: four handlers, `routes`
  (which proves it has four), and

  ```eiffel
  init(args: SEQ[STRING]): STEP[REST_SERVER]
    do
      REST_SERVER.start(args, 8090, routes)
    end

  step(s: REST_SERVER, e: EVENT): STEP[REST_SERVER]
    do
      s.serve(e)
    end
  ```

Only the contracts of the three packages reach `HELLO_API` when it is verified.

## Verify, build and run

From the repository root:

```powershell
gardelc --project examples/hello_api --strict
./build/hello_api_bin.exe        # on port 8090
./build/hello_api_bin.exe 9000   # on port 9000
```

The executable is built in `build/` under the current directory.

The service listens on `127.0.0.1` unless `GARDEL_LISTEN_ADDR` names another address
(`0.0.0.0` is every interface). It prints nothing while it runs. `Ctrl+C` stops it with exit
status `0`. An argument that is not a port from 1 to 65535 stops it with exit status `2` and
`grest: not a port: <argument>`. If the port is taken, it stops with exit status `1` and
`grest: listen failed`.

In another terminal:

```powershell
curl.exe -i http://127.0.0.1:8090/hello
curl.exe -i http://127.0.0.1:8090/hello/ana%20b
curl.exe -i -X POST --data "hi there" http://127.0.0.1:8090/echo
curl.exe -i http://127.0.0.1:8090/missing
```

```text
HTTP/1.1 200
Content-Type: application/json
Content-Length: 17

{"hello":"ana b"}
```

`cargo test --test packages a_service_on_grest` verifies the service and sends it requests over
real sockets, some cut across several writes.

## Scripted runs and replay

A reactive program (spec §11.6) can run from a script of events instead of the network.
`Send` and `Close` are written to standard error, with `%XX` for each byte that is not a letter
or digit:

```bash
GARDEL_EVENTS="Accepted:1 Received:1:GET%20/hello/ana%20HTTP/1.1%0D%0A%0D%0A Shutdown" ./build/hello_api_bin.exe
```

```text
net: Listen 8090
net: Send 1 HTTP%2F1%2E1%20200%0D%0AContent%2DType%3A%20application%2Fjson%0D%0AContent%2DLength%3A%2015%0D%0A%0D%0A%7B%22hello%22%3A%22ana%22%7D
net: Close 1
```

`GARDEL_RECORD=<file>` records a live run, and `GARDEL_REPLAY=<file>` runs it again from the
log and checks that each command and the exit status are the same.
