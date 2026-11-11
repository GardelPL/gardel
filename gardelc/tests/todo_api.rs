//! `examples/todo_api` (Task F.6): a REST service with a state, on `grest`. It verifies under
//! `--strict`, its store's invariant is a fact a client proves through the contracts alone,
//! and it serves create, read, replace and delete over real sockets.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn assert_ok(out: &Output) {
    assert!(out.status.success(), "{}", all(out));
    assert!(all(out).contains("0 errors, 0 warnings"), "{}", all(out));
}

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_todo_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).expect("temporary directory");
        Self(dir)
    }

    fn check(&self, project: &PathBuf) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&self.0)
            .env("GARDEL_CACHE_DIR", self.0.join("cache"))
            .args(["--project", &project.to_string_lossy(), "--strict"])
            .output()
            .expect("gardelc runs")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

/// A package holding the example's `TODO` and `TODO_STORE`, and a client module with a
/// routine for each of `claims`, which ensures it.
fn store_client(dir: &Dir, claims: &[(&str, &str)]) -> PathBuf {
    let project = dir.0.join("client");
    fs::create_dir_all(project.join("src")).expect("source directory");
    let gjson = root().join("gjson").canonicalize().expect("gjson exists");
    let gjson = gjson.to_string_lossy().replace('\\', "/").replace("//?/", "");
    fs::write(
        project.join("Gardel.toml"),
        format!("[project]\nname = \"store-client\"\nversion = \"0.1.0\"\n\n[dependencies]\ngjson = {{ path = \"{gjson}\" }}\n"),
    )
    .expect("manifest");
    for file in ["todo.gdl", "todo_store.gdl"] {
        fs::copy(root().join("examples/todo_api/src").join(file), project.join("src").join(file)).expect("the example's file");
    }
    let routines: Vec<String> = claims
        .iter()
        .map(|(tag, claim)| format!("  {tag}_holds(s: TODO_STORE, t: TODO, k: NAT): BOOL\n    do\n      True\n    ensure\n      {tag}: {claim}\n    end\n"))
        .collect();
    fs::write(project.join("src/client.gdl"), format!("functions CLIENT\n\nfeature {{ANY}}\n\n{}\nend\n", routines.join("\n")))
        .expect("client");
    project
}

/// What any state of the service satisfies, whatever requests led to it: each todo is kept
/// under its own id, no todo has an id at or past the next, and so an added todo leaves every
/// id past its own free. And what each change does: an added todo is found, a deleted one is
/// not, a replaced one has its new `done`, and the other todos stay.
#[test]
fn a_client_proves_the_store_invariant() {
    let dir = Dir::new("facts");
    let project = store_client(
        &dir,
        &[
            ("keyed", "s.find(k).is_some implies s.find(k).get.todo_id = k"),
            ("fresh", "k >= s.next_id implies s.find(k).is_none"),
            ("added_fresh", "(s.has_room and t.todo_id = s.next_id and k > s.next_id) implies s.add(t).find(k).is_none"),
            ("deleted_keep_next", "s.remove(k).next_id = s.next_id"),
            ("deleted_gone", "s.remove(k).find(k).is_none"),
            ("added_found", "(s.has_room and t.todo_id = s.next_id) implies s.add(t).find(t.todo_id).is_some"),
            ("others_kept", "(s.find(k).is_some and k != t.todo_id) implies s.remove(t.todo_id).find(k).is_some"),
            ("replaced_done", "s.find(t.todo_id).is_some implies (s.replace(t).find(t.todo_id).get.is_done = t.is_done)"),
        ],
    );
    assert_ok(&dir.check(&project));
}

#[test]
fn a_claim_past_the_store_invariant_does_not_verify() {
    let lies = [
        // A store may be empty.
        ("claim_found", "s.find(k).is_some"),
        // A deleted todo leaves a gap below the next id.
        ("claim_dense", "k < s.next_id implies s.find(k).is_some"),
        // The next id never goes back, so a store never reuses one.
        ("claim_reuse", "s.remove(k).next_id < s.next_id"),
        // A delete deletes one todo, not every other.
        ("claim_delete_all", "s.remove(t.todo_id).find(k).is_none"),
    ];
    let dir = Dir::new("lies");
    let out = dir.check(&store_client(&dir, &lies));
    assert!(!out.status.success(), "the lies must fail:\n{}", all(&out));
    for (tag, _) in lies {
        assert!(all(&out).contains(&format!("condition `{tag}` failed")), "{tag} must be refused:\n{}", all(&out));
    }
}

/// The service verifies, then answers a session of requests over real sockets, each on its
/// own connection, in the state the requests before it left.
#[test]
fn todo_api_verifies_and_serves_crud_over_sockets() {
    let dir = Dir::new("serve");
    assert_ok(&dir.check(&root().join("examples/todo_api")));

    // A port nothing holds: the one the system gives, let go of.
    let port = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a free port").local_addr().expect("its address").port();
    let mut server = Command::new(dir.0.join("build/todo_api_bin.exe"))
        .arg(port.to_string())
        .spawn()
        .expect("the service starts");
    let up = (0..50).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
    });
    // The whole session is one persistent connection: each answer is read by its length.
    let socket = std::cell::RefCell::new(None::<std::net::TcpStream>);
    let ask = |method: &str, path: &str, body: Option<&str>| -> String {
        let mut slot = socket.borrow_mut();
        let socket = slot.get_or_insert_with(|| std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect"));
        let request = match body {
            None => format!("{method} {path} HTTP/1.1\r\nHost: test\r\n\r\n"),
            Some(b) => format!("{method} {path} HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n{b}", b.len()),
        };
        socket.write_all(request.as_bytes()).expect("request");
        read_answer(socket)
    };
    let session: &[(&str, &str, Option<&str>)] = &[
        ("POST", "/todos", Some(r#"{"title":"milk"}"#)),
        ("POST", "/todos", Some(r#"{"title":"eggs","done":true}"#)),
        ("GET", "/todos", None),
        ("PUT", "/todos/1", Some(r#"{"title":"oat milk","done":true}"#)),
        ("DELETE", "/todos/2", None),
        ("GET", "/todos/2", None),
        ("POST", "/todos", Some(r#"{"title":""}"#)),
        ("POST", "/todos", Some("not json")),
        ("POST", "/todos", Some(r#"{"title":"tea"}"#)),
        ("GET", "/todos", None),
        ("GET", "/todos/3", None),
        ("DELETE", "/todos/2", None),
        ("PUT", "/todos/9", Some(r#"{"title":"x"}"#)),
        ("PATCH", "/todos/1", None),
    ];
    let answers: Vec<String> = if up { session.iter().map(|(m, p, b)| ask(m, p, *b)).collect() } else { vec![] };
    // After the last answer the server closes the connection.
    let closed = up && {
        let mut rest = Vec::new();
        socket.borrow_mut().as_mut().expect("the connection").read_to_end(&mut rest).map_or(false, |_| rest.is_empty())
    };
    drop(server.kill());
    drop(server.wait());
    assert!(up, "the service did not start listening");
    let json = |code: u32, body: &str| format!("HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let not_found = json(404, r#"{"error":"not found"}"#);
    let expected = [
        json(201, r#"{"done":false,"id":1,"title":"milk"}"#),
        json(201, r#"{"done":true,"id":2,"title":"eggs"}"#),
        json(200, r#"[{"done":false,"id":1,"title":"milk"},{"done":true,"id":2,"title":"eggs"}]"#),
        json(200, r#"{"done":true,"id":1,"title":"oat milk"}"#),
        "HTTP/1.1 204\r\nContent-Length: 0\r\n\r\n".to_string(),
        not_found.clone(),
        json(400, r#"{"error":"a todo is an object with a title of 1 to 200 bytes and a boolean done"}"#),
        json(400, r#"{"at":0,"error":"invalid JSON body"}"#),
        // The id of the deleted todo is not given again.
        json(201, r#"{"done":false,"id":3,"title":"tea"}"#),
        json(200, r#"[{"done":true,"id":1,"title":"oat milk"},{"done":false,"id":3,"title":"tea"}]"#),
        json(200, r#"{"done":false,"id":3,"title":"tea"}"#),
        not_found.clone(),
        not_found,
        // `ghttp` knows no `PATCH`, so the request never reaches the router, and the
        // connection is closed after the error.
        "HTTP/1.1 501\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_string(),
    ];
    for (i, (got, want)) in answers.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "request {i}: {:?}", session[i]);
    }
    assert!(closed, "the connection stays open after the error");
}

/// One answer from `socket`: its head, up to the blank line, and as many body bytes as its
/// `Content-Length` says. A connection that ends first gives what arrived.
pub fn read_answer(socket: &mut std::net::TcpStream) -> String {
    socket.set_read_timeout(Some(std::time::Duration::from_secs(10))).expect("timeout");
    let mut got: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    while !got.ends_with(b"\r\n\r\n") {
        match socket.read(&mut byte) {
            Ok(1) => got.push(byte[0]),
            _ => return String::from_utf8_lossy(&got).into_owned(),
        }
    }
    let head = String::from_utf8_lossy(&got).into_owned();
    let length = head
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .map_or(0, |(_, v)| v.trim().parse::<usize>().expect("a length"));
    let mut body = vec![0u8; length];
    socket.read_exact(&mut body).expect("the body");
    got.extend(body);
    String::from_utf8_lossy(&got).into_owned()
}
