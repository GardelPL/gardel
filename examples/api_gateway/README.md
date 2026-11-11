# API Gateway Example (`api_gateway`)

A verified gateway in front of another service, on [`grest`](../../grest)'s async handlers and
[`ghttp`](../../ghttp)'s client: `GET /todos/:id` asks the upstream, a
[`todo_api`](../todo_api) say, for the same path, and answers with what it answers. While a
request waits for the upstream, the gateway goes on serving every other request.

| Request | Answer |
| :--- | :--- |
| `GET /health` | `200`, `ok`, from the gateway itself |
| `GET /todos/:id` | the upstream's status, media type and body for `GET /todos/:id` |
| ... when the upstream cannot be reached, closes with no answer, or answers what cannot be read | `502`, `{"error":"upstream unavailable"}` |
| ... when the upstream has not answered in 4 to 5 seconds | `504`, `{"error":"upstream timed out"}` |
| ... when 32 requests already wait | `503`, `{"error":"busy"}` |
| `GET /todos/:id` with an id that is not a number | `404`, without asking the upstream |

## How it works

`fetch_todo` is an `on_await` route. It gets a tag from the server, makes a
[`GATEWAY_CALL`](src/gateway_call.gdl) under that tag, and returns
`REST_OUTCOME.waiting(state, tag, [Connect, SetTimer], 5, 504)`: the request waits, the
gateway connects to the upstream, and the server answers `504` on its fifth sweep if nothing
else does. The tag also names the `Connect`, and plus one, the call's timer, as timer 0 is the
server's sweep.

The gateway's other events go to `wake`:

- `Connected(tag, c)`: the call sends its request on `c`, written by `HTTP_REQUEST_WRITER`
  with `Connection: close`.
- `Received(c, bytes)`: the call feeds them to `HTTP_CLIENT.receive`, and, once the answer is
  whole, answers the waiting request by its tag with `REST_WAKE.answering`, closes `c` and
  cancels its timer.
- `Closed(c)` or `SendFailed(c)`: `HTTP_CLIENT.closed` gives what came of the answer, or the
  request is answered `502`.
- `ConnectFailed(tag)`: `502`.
- The call's timer, after 8 s: the call ends and its connection is closed. Its request was
  answered `504` by then, so the answer given here is dropped by the server, as any answer to a
  request that no longer waits is.

The state, [`GATEWAY_CALLS`](src/gateway_calls.gdl), is the upstream's port and the calls in
flight.

## What is proven

`gardelc --project examples/api_gateway --strict` proves, with only the contracts of `grest`,
`ghttp` and `gjson`:

- **At most 32 calls are upstream** (`GATEWAY_CALLS`'s invariant), and `wake` never adds one
  (`not_more`): only a request makes a call, and with 32 it is a `503`.
- **A request waits only under the tag it was given, and a call is made under it**
  (`fetch_todo`'s `own_tag`), so the server holds it.
- **Every wait has a timeout**, of 5 sweeps: `REST_OUTCOME.waiting` cannot be called without
  one from 1 to 30. A copy with `0` or `31` is refused (`timed`), which `cargo test` checks.
- **An answer goes only to a request a call was made for** (`wake`'s `answers_a_call`).
- **A closed upstream connection ends its call** (`wake`'s `freed`).
- **Each call's client side waits for its answer** (`GATEWAY_CALL`'s invariant), as
  `HTTP_CLIENT.receive` requires.
- From `grest`: the waiting table's limit, a closed client connection that frees its wait,
  a late answer that reaches no one, and the server's safety.

Not proven: that each call's timer is set, and cancelled when it ends; that an upstream's
answer goes to the request that asked for it rather than another call's (the tag is the
call's, but that the bytes on `c` are its call's answer is the runtime's). The socket test
checks both.

## Tests

`cargo test --test api_gateway` verifies the gateway, checks that a wait with no timeout or
one too long is refused, and runs it over real sockets against a fake upstream in the test,
which is also the first socket test of `ghttp`'s client:

- an answer relayed, its head and body read in two parts; a `404` relayed; an upstream that
  closes with no answer is a `502`;
- a slow upstream and a silent one: meanwhile, another connection gets `/health` and a relayed
  answer at once; both waiting requests get a `504` after 4 to 5 s; the slow upstream's answer
  at 6 s reaches no one, and its connection serves the next request; the silent upstream's
  connection is closed at 8 s;
- the request the upstream gets is the writer's, with the gateway's `Host` and a close;
- a dead upstream is a `502` at once.

## Verify, build and run

From the repository root:

```powershell
gardelc --project examples/todo_api --strict
gardelc --project examples/api_gateway --strict
./build/todo_api_bin.exe                 # on port 8091
./build/api_gateway_bin.exe              # on port 8092, in front of 8091
./build/api_gateway_bin.exe 9000 9001    # on port 9000, in front of 9001
```

In another terminal:

```powershell
curl.exe -i -X POST --data '{\"title\":\"milk\"}' http://127.0.0.1:8091/todos
curl.exe -i http://127.0.0.1:8092/todos/1
curl.exe -i http://127.0.0.1:8092/health
```

## Known limits

- The upstream is on `127.0.0.1`: the runtime connects to an IPv4 address, and resolves no
  names.
- One call per request, and one request per call: the gateway does not pool its upstream
  connections.
- An upstream's answer longer than 64 KiB, or not UTF-8, is a `502`, as a `grest` response
  holds text of at most 64 KiB. Its fields other than `Content-Type` are not passed on, and a
  media type other than JSON is passed on as plain text.
- A connection whose request waits reads nothing else until its answer: requests pipelined
  after it are not answered and the connection closes after the answer (`grest`'s limit).
