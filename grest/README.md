# Gardel REST (`grest`)

A verified REST layer for Gardel, built on `ghttp` and `gjson` in Task F.6 of
[plan/libraries/verified_libraries.md](../plan/libraries/verified_libraries.md). It is in progress:
this version has the URL layer, typed requests and responses, and a router whose routes hold their
handlers. The server loop comes next.

```eiffel
functions API

feature {ANY}

  user(r: REST_REQUEST): HTTP_RESPONSE
    do
      match r.param("id")
        case Some(id) then REST_RESPONSE.ok_json(...)
        case None then REST_RESPONSE.not_found
      end
    end

  add_user(r: REST_REQUEST, body: JSON): HTTP_RESPONSE
    do
      REST_RESPONSE.created_json(body)
    end

  router: REST_ROUTER
    do
      REST_ROUTER.empty
        .on_get("/users/:id", user)
        .on_json(Post, "/users", add_user)
    end

end
```

`router.dispatch(req)` answers an `HTTP_REQUEST`. A handler is a pure function, and a routine's
name is one. Handlers belong in a `functions` or `program` module: in a `module`, a lambda that calls
a sibling routine is not yet translated.

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
| `REST_ROUTER` | Up to 64 routes, tried in the order they were added. `on_get`, `on_post`, `on_put`, `on_delete` and `on_json` add one. `dispatch` answers a request, and `invalid_route` names the first pattern that is not valid. |
| `REST_ROUTE` | A method, a pattern and a handler, `(r: REST_REQUEST) -> HTTP_RESPONSE`. `with_json` makes a route whose handler is also given the body as `JSON`. |
| `REST_REQUEST` | What a handler sees: `method`, `param` and `param_nat` (path parameters), `query`, `header` (ignoring the case of the name), `body`, `json_body`, and the `http` request itself. |
| `REST_RESPONSE` | `json`, `ok_json`, `created_json`, `text`, `ok_text` and `no_content`, and errors as a JSON object: `error(status, message)`, `bad_request`, `not_found`, `method_not_allowed`, `bad_target` and `bad_body`. A JSON body is sent with `Content-Type: application/json`, and text with `text/plain; charset=utf-8`. |
| `URL_TEXT` | Percent decoding (RFC 3986 section 2.1). In a query, `+` is also a space. A `%` with no two hex digits is `BadEscape`, and bytes that are not UTF-8 are `NotUtf8`. |
| `REQUEST_TARGET` | `/users/42?q=a+b` as segments `users`, `42` and the field `q` = `a b`. A segment is decoded after the split, so `%2F` stays inside it. Empty query fields are skipped, and a field with no `=` has an empty value. |
| `PATH_PATTERN`, `PATH_SEGMENT` | `/users/:id`: a segment that starts with `:` is a parameter, and the others must equal the request's decoded segment. |
| `URL_PARAM` | A decoded name and value, and `find`, the value of the first one with a name. |
| `URL_ERROR` | Why a target was refused, each one a `400`, with a short `reason`. |

## How a request is answered

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
`ok_json`, a route that never answers `400`, and the limits less one.

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
