# Gardel REST (`grest`)

A verified REST layer for Gardel, built on `ghttp` and `gjson` in Task F.6 of
[plan/libraries/verified_libraries.md](../plan/libraries/verified_libraries.md). It has the URL layer,
typed requests and responses, a router whose routes hold their handlers, and a server. A service
is its state, its handlers, its routes and two lines. The router and the server take the type of
the state as a parameter, and a handler that changes the state returns the next one with its
answer, a `REST_OUTCOME[S]`:

```eiffel
reactive program API

feature {ANY}

  -- Reads the state.
  user(s: STORE, r: REST_REQUEST): HTTP_RESPONSE
    do
      match r.param_nat("id")
        case Some(id) then REST_RESPONSE.ok_json(...)
        case None then REST_RESPONSE.not_found
      end
    end

  -- Changes it.
  add_user(s: STORE, r: REST_REQUEST, body: JSON): REST_OUTCOME[STORE]
    do
      REST_OUTCOME.make(s.with(body), REST_RESPONSE.created_json(body))
    end

  router: REST_ROUTER[STORE]
    do
      REST_ROUTER.empty
        .on_view("/users/:id", user)
        .on_json(Post, "/users", add_user)
    end

  init(args: SEQ[STRING]): STEP[REST_SERVER[STORE]]
    do
      REST_SERVER.start(args, 8090, router, STORE.empty)
    end

  step(s: REST_SERVER[STORE], e: EVENT): STEP[REST_SERVER[STORE]]
    do
      s.serve(e)
    end

end
```

`REST_SERVER.start` listens on the port its first argument names, or on `8090` when there is
none. A route whose pattern is not valid, or an argument that is not a port, stops the program
with exit status 2 and a message, before it listens.
[`examples/todo_api`](../examples/todo_api) is a service with a state, and
[`examples/hello_api`](../examples/hello_api) one of four routes whose handlers do not look at
it, so its state is a `NAT` that stays `0`.

A handler is a pure function, and a routine's name is one. Handlers belong in a `functions` or
`program` module: in a `module`, a lambda that calls a sibling routine is not yet translated.
`router.dispatch(s, req)` answers one `HTTP_REQUEST` in the state `s` without a server, as a
test may.

The state is whatever type the service gives. Its invariant is proven of every value of the
type, so it holds after every request with no proof in `grest`. `grest` cannot make a state of
its own: it can only pass on the one a handler gave back, or keep the one it had.

A client lists `ghttp` and `gjson` beside `grest` in its `Gardel.toml`, as a package sees only the
packages it names:

```toml
[dependencies]
grest = { path = "../grest" }
ghttp = { path = "../ghttp" }
gjson = { path = "../gjson" }
```

## Modules

| Module | What it does |
| :--- | :--- |
| `REST_SERVER[S]` | The connections, the router and the state of a service. `start` checks the routes and the port and listens; `serve` is the step for one event, and `state` the service's state. Up to 64 connections are open at once, a request is answered when its bytes are complete, the state becomes the one its handler gave back, and the connection is closed after the answer. |
| `REST_ROUTER[S]` | Up to 64 routes, tried in the order they were added. `on(m, p, h)` adds one whose handler gives the next state, `on_json` one whose handler is also given the body as `JSON`, `on_view` a `GET` route that reads the state, and `on_get`, `on_post`, `on_put` and `on_delete` routes whose handlers do not look at it. `dispatch` answers a request in a state, and `invalid_route` names the first pattern that is not valid. |
| `REST_ROUTE[S]` | A method, a pattern and a handler, `(s: S, r: REST_REQUEST) -> REST_OUTCOME[S]`. `plain`, `viewing` and `with_json` make one from a handler of another shape. |
| `REST_OUTCOME[S]` | What a handler gives back: the next `state` and the `response`. |
| `REST_COMMANDS` | The commands a server step may give (`for_conn`, `for_none`, `shaped`), and what `start` reads from its arguments. They do not depend on the state, so they are apart from the generic server. |
| `REST_REQUEST` | What a handler sees: `method`, `param` and `param_nat` (path parameters), `query`, `header` (ignoring the case of the name), `body`, `json_body`, and the `http` request itself. |
| `REST_RESPONSE` | `json`, `ok_json`, `created_json`, `text`, `ok_text` and `no_content`, and errors as a JSON object: `error(status, message)`, `bad_request`, `not_found`, `method_not_allowed`, `bad_target` and `bad_body`. A JSON body is sent with `Content-Type: application/json`, and text with `text/plain; charset=utf-8`. |
| `URL_TEXT` | Percent decoding (RFC 3986 section 2.1). In a query, `+` is also a space. A `%` with no two hex digits is `BadEscape`, and bytes that are not UTF-8 are `NotUtf8`. |
| `REQUEST_TARGET` | `/users/42?q=a+b` as segments `users`, `42` and the field `q` = `a b`. A segment is decoded after the split, so `%2F` stays inside it. Empty query fields are skipped, and a field with no `=` has an empty value. |
| `PATH_PATTERN`, `PATH_SEGMENT` | `/users/:id`: a segment that starts with `:` is a parameter, and the others must equal the request's decoded segment. |
| `URL_PARAM` | A decoded name and value, and `find`, the value of the first one with a name. |
| `URL_ERROR` | Why a target was refused, each one a `400`, with a short `reason`. |

## How a request is answered

`REST_SERVER.serve` admits a connection while fewer than 64 are open, and closes it at once
otherwise. It gives each connection's bytes to `ghttp`'s parser until they make a request, then
sends the router's answer and closes the connection. Bytes that cannot be a request are answered
with `ghttp`'s status for the error (`400`, `413`, `414`, `431`, `501` or `505`, with no body) and the connection is closed.

`REST_ROUTER.dispatch`:

1. A target that cannot be read is a `400`, `{"error":"<reason>"}`.
2. The first route whose pattern matches the path and whose method is the request's answers it. A
   `HEAD` request is answered by a `GET` route.
3. A path that a route matches, with no route for the method, is a `405`.
4. Any other path is a `404`, `{"error":"not found"}`.

A route made by `on_json` or `REST_ROUTE.with_json` reads the body as JSON first. A body that is not
one JSON value, or no body, is a `400`, `{"at":<byte>,"error":"invalid JSON body"}`, and its handler
is not called.

A pattern is split as a target is: `/` is one empty segment, and `/users/` has two, `users` and an
empty one, so it does not match `/users`. Patterns are written decoded.

## What is proven

`gardelc --project grest --strict` proves:

- **Limits.** A parsed target has at most `REQUEST_TARGET.max_segments` (16) segments and
  `max_fields` (32) query fields. A target with more is refused, so the work of a match is bounded
  whatever a client sends. A router holds at most 64 routes, and a request at most 16 path
  parameters.
- **A match gives one value per parameter.** If `pattern.match_target(t)` is `Some(ps)`, then `ps`
  has exactly `pattern.param_count` entries, never more than 16, and `t` has as many segments as the
  pattern.
- **Server safety,** for every state and every event, by `serve`'s contract `REST_SERVER.within`:
  - the limit on connections is the same after a step, and the number open never exceeds it;
  - an event about a connection does nothing, closes it, or sends it one answer and then closes
    it, so a request, or bytes that cannot be one, get one answer, and no connection hears an
    answer meant for another;
  - any other event names no connection, and a `Shutdown` stops the server.

  `start` does not start a router with a pattern that is not valid.
- **The state is the handlers'.** `REST_OUTCOME.make(s, resp).state` is `s`. A target that
  cannot be read, and any request to a router with no routes, keep the state they were given.
- **Statuses.** A target that cannot be read is a `400` whatever the routes. A router with no
  routes answers `404`. `bad_body` and `bad_target` are `400`. `error(st, m)` has the status `st`,
  even when its body cannot be built. `ok_json(v)` is a `200`, or a `500` if `v`'s text is larger
  than a response can hold (64 KiB).
- **Decoding never grows text,** and text with no `%` (nor `+`, in a query) decodes to itself,
  with no copy.
- **Only a valid pattern matches.** Any text gives a `PATH_PATTERN`, so a route can be written
  with no precondition to prove. A pattern that does not start with `/`, has more than 16 segments,
  holds a `?` or a `%`, or has a parameter with no name is not valid, and it matches nothing.
  `invalid_route` finds it, so a server can refuse to start.

`gardelc/tests/grest.rs` checks that a client proves these through the contracts alone, and that
claims past them are refused: a `404` for every request to an empty router, a `200` for every
`ok_json`, a route that never answers `400`, the limits less one, a server that never stops, one
that always answers, one that is never full, and one that starts with any argument. It also runs
a scripted service: a bad route and a bad port stop it with exit status 2, with 64
connections open the next is closed and its request is not answered, and a count kept as the
state goes up with each request that reaches its handler, in the order the requests complete,
and not with one that is refused.

## Tests

`grest/tests` is a package that depends on `grest`, `ghttp` and `gjson`. `GREST_API` is a small API
of nine routes, and its program reads the cases of `grest/tests/cases.txt` on standard input. Each
line is a decoding, a target, a pattern, a match, a request to that API (or the bytes of its
answer), and what it must give:

```text
gardelc --project grest/tests --strict
grest/tests/build/grest_test_bin.exe < grest/tests/cases.txt
```

Exit code 0 means every case holds. `cargo test --test grest` does both.

## Known limits

- A `405` has no `Allow` header, as an `HTTP_RESPONSE` has no headers but `Content-Type` and
  `Content-Length`.
- A `HEAD` request gets the `GET` route's body as well as its headers. The server closes the
  connection after each answer, so a client is not confused about where the answer ends.
- A response body is at most 64 KiB, `ghttp`'s limit.
- One request per connection: the connection is closed after its answer, and bytes after the
  request are dropped. Persistent connections come with `ghttp`'s client (F.8).
- The state is the whole service's: each request sees the state the one before it left, and two
  requests are never handled at once. A handler that must wait for something else, such as
  another service, comes with F.8.
