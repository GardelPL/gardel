# Todo API Example (`todo_api`)

A verified REST service with a state, on the [`grest`](../../grest) package: todos, created,
read, replaced and deleted over JSON. The state is a [`TODO_STORE`](src/todo_store.gdl), and
each handler takes it and gives back the next one with its answer.

| Request | Answer |
| :--- | :--- |
| `GET /todos` | `200`, every todo, in ascending order of id: `[{"done":false,"id":1,"title":"milk"}]` |
| `POST /todos` with `{"title":"milk"}` or `{"title":"milk","done":true}` | `201`, the todo with its new id |
| `GET /todos/:id` | `200`, the todo, or `404` |
| `PUT /todos/:id` with a todo | `200`, the todo as it now is, or `404` if there is none with that id |
| `DELETE /todos/:id` | `204`, or `404` |
| A body that is not JSON | `400`, `{"at":<byte>,"error":"invalid JSON body"}` |
| JSON that is not a todo | `400`: a title is a string of 1 to 200 bytes, and `done`, if given, a boolean |
| Another method on one of these paths | `405` |
| Any other path | `404` |

## What is proven

`gardelc --project examples/todo_api --strict` proves, with only the contracts of `grest`,
`ghttp` and `gjson`:

- **The store's invariant, after every request, whatever the requests were.** Each todo is kept
  under its own id, and no todo has an id at or past the next one, so a new todo never takes the
  id of another, even one that was deleted. The next id is at most 1000001; once no id is left,
  `POST` is a `507` and the store is unchanged. The invariant is a fact of every `TODO_STORE`, so
  `REST_SERVER[TODO_STORE]` holds one after each step without a proof in `grest`. It rests on
  `MAP`'s laws (what `get` says after a `put` or a `remove`).
- **Each todo** has an id from 1 to 1000000 and a title of 1 to 200 bytes.
- **What each change does.** `add` makes the new todo found, `replace` gives the todo its new
  `done`, and `remove` makes the id not found, and each leaves every other id as found or not
  found as it was.
- **The handlers.** `create_todo` moves the next id by one at most, and `update_todo` and
  `delete_todo` leave it alone. `PUT` replaces only a todo that is there.
- **The state a request leaves is the one its handler gave back,** from `REST_SERVER`'s
  contract, so the next request sees it.
- **The server's safety,** from `REST_SERVER`'s contract: each request is answered once, in
  order, at most 64 connections are open, and no connection gets more than 100 answers.

`cargo test --test todo_api` checks that a client proves the invariant and what each change
does through the contracts alone, and that claims past them are refused (that a store holds a
todo, that the ids below the next are all taken, that a delete moves the next id back, that a
delete removes every todo). It then runs a session over real sockets.

## Verify, build and run

From the repository root:

```powershell
gardelc --project examples/todo_api --strict
./build/todo_api_bin.exe        # on port 8091
./build/todo_api_bin.exe 9000   # on port 9000
```

In another terminal:

```powershell
curl.exe -i -X POST --data '{\"title\":\"milk\"}' http://127.0.0.1:8091/todos
curl.exe -i http://127.0.0.1:8091/todos
curl.exe -i -X PUT --data '{\"title\":\"oat milk\",\"done\":true}' http://127.0.0.1:8091/todos/1
curl.exe -i -X DELETE http://127.0.0.1:8091/todos/1
```

## Known limits

- The todos live in memory and go when the service stops.
- `MAP` walks a list, so each request takes time linear in the number of todos, and
  `GET /todos` is a `500` once the todos' text is longer than a response holds (64 KiB, about
  1500 todos with short titles).
- `ghttp` knows no `PATCH`, so a change is a whole `PUT`.
