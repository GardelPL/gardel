//! `examples/api_gateway` (Task F.8, step 4): a `grest` route that waits for another service,
//! over `ghttp`'s client. It verifies under `--strict`, a wait without a timeout is refused, and
//! over real sockets it relays an upstream's answers, answers others while one waits, times a
//! slow upstream out, drops its late answer, closes a silent one, and answers `502` when the
//! upstream is dead.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
        let dir = std::env::temp_dir().join(format!("gardelc_gateway_{}_{name}", std::process::id()));
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

/// A port nothing holds: the one the system gives, let go of.
fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0)).expect("a free port").local_addr().expect("its address").port()
}

/// What the fake upstream saw: each request head, and for `/todos/5`, after how long the
/// gateway closed the connection.
#[derive(Default)]
struct Seen {
    heads: Vec<String>,
    silent_closed_after: Option<Duration>,
}

fn head_of(socket: &mut TcpStream) -> String {
    let mut got: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    while !got.ends_with(b"\r\n\r\n") {
        match socket.read(&mut byte) {
            Ok(1) => got.push(byte[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&got).into_owned()
}

fn json(code: u32, body: &str) -> String {
    format!("HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len())
}

/// A fake upstream on a free port, by path:
/// - `/todos/1`: a todo, its head and body in two writes;
/// - `/todos/2`: the same, after 6 s, past the gateway's timeout;
/// - `/todos/3`: closes with no answer;
/// - `/todos/4`: a `404`;
/// - `/todos/5`: never answers, and notes when the gateway closes.
fn upstream() -> (u16, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream listens");
    let port = listener.local_addr().expect("its address").port();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let log = seen.clone();
    thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else { continue };
            let log = log.clone();
            thread::spawn(move || {
                let start = Instant::now();
                let head = head_of(&mut socket);
                log.lock().expect("the log").heads.push(head.clone());
                let path = head.split(' ').nth(1).unwrap_or("").to_string();
                let todo = json(200, r#"{"done":false,"id":1,"title":"milk"}"#);
                match path.as_str() {
                    "/todos/1" => {
                        let (h, b) = todo.split_at(todo.find("\r\n\r\n").expect("a head") + 4);
                        drop(socket.write_all(h.as_bytes()));
                        thread::sleep(Duration::from_millis(100));
                        drop(socket.write_all(b.as_bytes()));
                    }
                    "/todos/2" => {
                        thread::sleep(Duration::from_secs(6));
                        drop(socket.write_all(todo.as_bytes()));
                    }
                    "/todos/4" => drop(socket.write_all(json(404, r#"{"error":"not found"}"#).as_bytes())),
                    "/todos/5" => {
                        let mut rest = Vec::new();
                        drop(socket.read_to_end(&mut rest));
                        log.lock().expect("the log").silent_closed_after = Some(start.elapsed());
                    }
                    _ => {}
                }
                // The gateway asks to close after the answer: the upstream does.
            });
        }
    });
    (port, seen)
}

/// One answer from `socket`: its head and as many body bytes as its `Content-Length` says.
fn read_answer(socket: &mut TcpStream) -> String {
    socket.set_read_timeout(Some(Duration::from_secs(15))).expect("timeout");
    let mut got = head_of(socket).into_bytes();
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

fn send(socket: &mut TcpStream, path: &str) {
    socket.write_all(format!("GET {path} HTTP/1.1\r\nHost: test\r\n\r\n").as_bytes()).expect("request");
}

fn connect(port: u16) -> TcpStream {
    TcpStream::connect(("127.0.0.1", port)).expect("connect")
}

/// The gateway, on `port`, in front of the upstream on `up`, once it listens.
fn gateway(dir: &Dir, port: u16, up: u16) -> Child {
    let child = Command::new(dir.0.join("build/api_gateway_bin.exe"))
        .args([port.to_string(), up.to_string()])
        .spawn()
        .expect("the gateway starts");
    let listening = (0..50).any(|_| {
        thread::sleep(Duration::from_millis(100));
        TcpStream::connect(("127.0.0.1", port)).is_ok()
    });
    assert!(listening, "the gateway did not start listening");
    child
}

struct Killed(Child);

impl Drop for Killed {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

/// The example, as a project in `dir`, with `edit` made to its program.
fn edited(dir: &Dir, name: &str, edit: (&str, &str)) -> PathBuf {
    let project = dir.0.join(name);
    fs::create_dir_all(project.join("src")).expect("source directory");
    let package = |p: &str| {
        let path = root().join(p).canonicalize().expect("the package exists");
        path.to_string_lossy().replace('\\', "/").replace("//?/", "")
    };
    fs::write(
        project.join("Gardel.toml"),
        format!(
            "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[dependencies]\ngrest = {{ path = \"{}\" }}\nghttp = {{ path = \"{}\" }}\ngjson = {{ path = \"{}\" }}\n",
            package("grest"),
            package("ghttp"),
            package("gjson")
        ),
    )
    .expect("manifest");
    for file in ["api_gateway.gdl", "gateway_call.gdl", "gateway_calls.gdl"] {
        let text = fs::read_to_string(root().join("examples/api_gateway/src").join(file)).expect("the example's file");
        let text = if file == "api_gateway.gdl" {
            assert_eq!(text.matches(edit.0).count(), 1, "the edit must apply once");
            text.replace(edit.0, edit.1)
        } else {
            text
        };
        fs::write(project.join("src").join(file), text).expect("the copy");
    }
    project
}

#[test]
fn api_gateway_verifies_and_waits_for_its_upstream_over_sockets() {
    let dir = Dir::new("serve");
    assert_ok(&dir.check(&root().join("examples/api_gateway")));

    let (up, seen) = upstream();
    let port = free_port();
    let _gateway = Killed(gateway(&dir, port, up));

    // Relayed: the upstream's status, media type and body, read in two parts.
    let mut a = connect(port);
    send(&mut a, "/todos/1");
    assert_eq!(read_answer(&mut a), json(200, r#"{"done":false,"id":1,"title":"milk"}"#));
    send(&mut a, "/todos/4");
    assert_eq!(read_answer(&mut a), json(404, r#"{"error":"not found"}"#));
    // An upstream that closes with no answer.
    send(&mut a, "/todos/3");
    assert_eq!(read_answer(&mut a), json(502, r#"{"error":"upstream unavailable"}"#));
    // A path id that is not a number never goes upstream.
    send(&mut a, "/todos/x");
    assert_eq!(read_answer(&mut a), json(404, r#"{"error":"not found"}"#));

    // A slow upstream and a silent one, and meanwhile the gateway answers others.
    let t0 = Instant::now();
    let mut slow = connect(port);
    send(&mut slow, "/todos/2");
    let mut silent = connect(port);
    send(&mut silent, "/todos/5");
    thread::sleep(Duration::from_millis(300));
    let mut other = connect(port);
    send(&mut other, "/health");
    assert_eq!(read_answer(&mut other), "HTTP/1.1 200\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 2\r\n\r\nok");
    let health_at = t0.elapsed();
    send(&mut other, "/todos/1");
    assert_eq!(read_answer(&mut other), json(200, r#"{"done":false,"id":1,"title":"milk"}"#));
    let timed_out = json(504, r#"{"error":"upstream timed out"}"#);
    assert_eq!(read_answer(&mut slow), timed_out);
    let slow_at = t0.elapsed();
    assert_eq!(read_answer(&mut silent), timed_out);
    assert!(health_at < Duration::from_secs(2), "another request waited for the upstream: {health_at:?}");
    assert!(slow_at >= Duration::from_secs(3) && slow_at <= Duration::from_secs(7), "the timeout came after {slow_at:?}");

    // The slow upstream answers at 6 s, too late: the answer reaches no one, and the
    // connection that waited serves the next request.
    thread::sleep(Duration::from_secs(7).saturating_sub(t0.elapsed()));
    send(&mut slow, "/health");
    assert_eq!(read_answer(&mut slow), "HTTP/1.1 200\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 2\r\n\r\nok");

    // The silent upstream's connection is closed by the call's timer, at 8 s.
    let closed = (0..60).find_map(|_| {
        thread::sleep(Duration::from_millis(100));
        seen.lock().expect("the log").silent_closed_after
    });
    let closed = closed.expect("the gateway never closed the silent upstream");
    assert!(closed >= Duration::from_secs(7) && closed <= Duration::from_secs(10), "closed after {closed:?}");

    // What the upstream was sent: `ghttp`'s writer, with the gateway's host and a close.
    let heads = seen.lock().expect("the log").heads.clone();
    let want = format!("GET /todos/1 HTTP/1.1\r\nHost: 127.0.0.1:{up}\r\nConnection: close\r\n\r\n");
    assert!(heads.contains(&want), "the upstream got {heads:?}");
    assert_eq!(heads.len(), 6, "the upstream got {heads:?}");

    // A dead upstream: nothing listens on its port.
    let dead = free_port();
    let port = free_port();
    let _dead_gateway = Killed(gateway(&dir, port, dead));
    let mut b = connect(port);
    let t1 = Instant::now();
    send(&mut b, "/todos/1");
    assert_eq!(read_answer(&mut b), json(502, r#"{"error":"upstream unavailable"}"#));
    assert!(t1.elapsed() < Duration::from_secs(4), "the failed connect took {:?}", t1.elapsed());
}

/// The verifier refuses a wait with no timeout, and one longer than 30 sweeps.
#[test]
fn a_wait_without_a_timeout_is_refused() {
    let dir = Dir::new("untimed");
    for (name, sweeps) in [("untimed", "0"), ("too-long", "31")] {
        let project = edited(&dir, name, ("5, upstream_timed_out)", &format!("{sweeps}, upstream_timed_out)")));
        let out = dir.check(&project);
        assert!(!out.status.success(), "{name} must fail:\n{}", all(&out));
        assert!(all(&out).contains("`timed`"), "{name} must be refused for its timeout:\n{}", all(&out));
    }
}
