//! `grest` (Task F.6): the package verifies, every case of `grest/tests/cases.txt` holds in C,
//! and a client proves the limits of a request target and a route match, and the statuses a
//! router and the responses give, through the contracts alone, while claims past them are
//! refused.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

struct Project {
    dir: PathBuf,
}

impl Project {
    /// A project in a directory of its own that depends on `grest`, and on `ghttp` and
    /// `gjson` for the types of its requests and bodies.
    fn new(name: &str, source: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_grest_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        let path = |p: &str| {
            let full = root().join(p).canonicalize().expect("the package exists");
            full.to_string_lossy().replace('\\', "/").replace("//?/", "")
        };
        let manifest = format!(
            "[project]\nname = \"{}\"\nversion = \"0.1.0\"\n\n[dependencies]\ngrest = {{ path = \"{}\" }}\nghttp = {{ path = \"{}\" }}\ngjson = {{ path = \"{}\" }}\n",
            name.replace('_', "-"),
            path("grest"),
            path("ghttp"),
            path("gjson")
        );
        fs::write(dir.join("Gardel.toml"), manifest).expect("manifest");
        fs::write(dir.join("src").join("client.gdl"), source).expect("source file");
        Self { dir }
    }

    fn check(&self) -> Output {
        check_at(&self.dir, &self.dir.join("cache"))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

fn check_at(dir: &Path, cache: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(dir)
        .env("GARDEL_CACHE_DIR", cache)
        .args(["--project", ".", "--strict"])
        .output()
        .expect("gardelc runs")
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn assert_ok(out: &Output) {
    assert!(out.status.success(), "{}", all(out));
    assert!(all(out).contains("0 errors, 0 warnings"), "{}", all(out));
}

/// The package and its test package verify, and the test program accepts every case:
/// decoding in a path and in a query, bad escapes and bytes that are not UTF-8, targets at
/// and past the limits, patterns, and matches.
#[test]
fn grest_verifies_and_holds_every_case() {
    let cache = std::env::temp_dir().join(format!("gardelc_grest_{}_cache", std::process::id()));
    drop(fs::remove_dir_all(&cache));
    assert_ok(&check_at(&root().join("grest"), &cache));
    let tests = root().join("grest/tests");
    assert_ok(&check_at(&tests, &cache));
    let cases = fs::read(tests.join("cases.txt")).expect("cases.txt");
    let mut run = Command::new(tests.join("build/grest_test_bin.exe"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the suite starts");
    run.stdin.take().expect("stdin").write_all(&cases).expect("cases are sent");
    let done = run.wait_with_output().expect("the suite ends");
    assert!(done.status.success(), "exit {:?}, failing case: {}", done.status.code(), String::from_utf8_lossy(&done.stdout));
    drop(fs::remove_dir_all(&cache));
}

fn client(claims: &str) -> String {
    format!(
        "functions CLIENT

feature {{ANY}}

  f(p: STRING, t: STRING, s: STRING): BOOL
    do
      True
    ensure
{claims}
    end

end
"
    )
}

/// What a client knows from the contracts alone: a parsed target is within its limits, a
/// match gives one value per parameter, and text with nothing to decode is itself.
#[test]
fn a_client_proves_the_limits_through_the_contracts() {
    let facts = client(
        "      segments: REQUEST_TARGET.parse(t).is_ok implies REQUEST_TARGET.parse(t).get.segment_count <= 16
      fields: REQUEST_TARGET.parse(t).is_ok implies REQUEST_TARGET.parse(t).get.field_count <= 32
      params: (REQUEST_TARGET.parse(t).is_ok and
          PATH_PATTERN.parse(p).match_target(REQUEST_TARGET.parse(t).get).is_some) implies
        PATH_PATTERN.parse(p).match_target(REQUEST_TARGET.parse(t).get).get.length = PATH_PATTERN.parse(p).param_count
      bounded: (REQUEST_TARGET.parse(t).is_ok and
          PATH_PATTERN.parse(p).match_target(REQUEST_TARGET.parse(t).get).is_some) implies
        PATH_PATTERN.parse(p).match_target(REQUEST_TARGET.parse(t).get).get.length <= 16
      plain: (forall k: NAT. k < s.byte_length implies LIST_OPS.at(s.byte_model, k) != 37) implies
        URL_TEXT.decode(s, False).get.equals(s)
      shorter: URL_TEXT.decode(s, True).is_ok implies URL_TEXT.decode(s, True).get.byte_length <= s.byte_length",
    );
    assert_ok(&Project::new("facts", &facts).check());
}

#[test]
fn a_claim_past_the_limits_does_not_verify() {
    let lies = [
        ("segments", "      claim: REQUEST_TARGET.parse(t).is_ok implies REQUEST_TARGET.parse(t).get.segment_count <= 15"),
        ("fields", "      claim: REQUEST_TARGET.parse(t).is_ok implies REQUEST_TARGET.parse(t).get.field_count <= 31"),
        // In a query `+` is decoded, so text with no `%` is not always itself.
        (
            "plus",
            "      claim: (forall k: NAT. k < s.byte_length implies LIST_OPS.at(s.byte_model, k) != 37) implies\n        URL_TEXT.decode(s, True).get.equals(s)",
        ),
    ];
    for (name, claim) in lies {
        let out = Project::new(&format!("lie_{name}"), &client(claim)).check();
        assert!(!out.status.success(), "{name} must fail:\n{}", all(&out));
        assert!(all(&out).contains("condition `claim` failed"), "{name} must refuse the claim:\n{}", all(&out));
    }
}

fn rest_client(claims: &str) -> String {
    format!(
        "functions CLIENT

feature {{ANY}}

  f(req: HTTP_REQUEST, r: REST_ROUTER[NAT], n: NAT, v: JSON, e: JSON_ERROR, u: URL_ERROR, s: STRING): BOOL
    do
      True
    ensure
{claims}
    end

end
"
    )
}

/// What a client knows of a router and the responses: a target that cannot be read is a
/// `400` whatever the routes, a router with no routes answers `404`, a body that is not JSON
/// is a `400`, and an error keeps its status, even one `HTTP_STATUS` has no constructor for.
#[test]
fn a_client_proves_the_statuses_through_the_contracts() {
    let facts = rest_client(
        "      bad_target: not REQUEST_TARGET.parse(req.target).is_ok implies r.dispatch(n, req).response.response_status_code = 400
      no_routes: REQUEST_TARGET.parse(req.target).is_ok implies REST_ROUTER.empty.dispatch(n, req).response.response_status_code = 404
      bad_body: REST_RESPONSE.bad_body(e).response_status_code = 400
      url: REST_RESPONSE.bad_target(u).response_status_code = 400
      conflict: REST_RESPONSE.error(HTTP_STATUS.make(409), s).response_status_code = 409
      json: REST_RESPONSE.ok_json(v).response_status_code = 200 or REST_RESPONSE.ok_json(v).response_status_code = 500
      created: REST_RESPONSE.created_json(v).response_status_code = 201 or REST_RESPONSE.created_json(v).response_status_code = 500
      gone: REST_RESPONSE.no_content.response_body_length = 0
      routes: r.route_count <= 64
      keeps: not REQUEST_TARGET.parse(req.target).is_ok implies r.dispatch(n, req).state = n
      outcome: REST_OUTCOME.make(n, REST_RESPONSE.not_found).state = n",
    );
    assert_ok(&Project::new("statuses", &facts).check());
}

#[test]
fn a_claim_past_the_statuses_does_not_verify() {
    let lies = [
        // A target that cannot be read is a `400`, even with no routes.
        ("no_routes", "      claim: REST_ROUTER.empty.dispatch(n, req).response.response_status_code = 404"),
        // A value too large to send is a `500`.
        ("json", "      claim: REST_RESPONSE.ok_json(v).response_status_code = 200"),
        // A route may answer with any status, `400` too.
        ("routed", "      claim: REQUEST_TARGET.parse(req.target).is_ok implies r.dispatch(n, req).response.response_status_code != 400"),
        // A route's handler may change the state.
        ("kept", "      claim: r.dispatch(n, req).state = n"),
    ];
    for (name, claim) in lies {
        let out = Project::new(&format!("rest_lie_{name}"), &rest_client(claim)).check();
        assert!(!out.status.success(), "{name} must fail:\n{}", all(&out));
        assert!(all(&out).contains("condition `claim` failed"), "{name} must refuse the claim:\n{}", all(&out));
    }
}

fn server_client(claims: &str) -> String {
    format!(
        "functions CLIENT

feature {{ANY}}

  f(s: REST_SERVER[NAT], e: EVENT, args: SEQ[STRING], r: REST_ROUTER[NAT], n: NAT): BOOL
    do
      True
    ensure
{claims}
    end

end
"
    )
}

/// What a client knows of `REST_SERVER` for every state and event: the limit on connections
/// holds and never changes, an event about a connection does nothing, closes it, or sends it
/// one answer and closes it, any other event names no connection, a `Shutdown` stops, a
/// router with a pattern that is not valid does not start, and the next state is the one the
/// handler of a completed request gave back, or the same.
#[test]
fn a_client_proves_the_server_safety_through_the_contracts() {
    let facts = server_client(
        "      within: REST_SERVER.within(s, e, s.serve(e))
      stops: e.is_shutdown implies s.serve(e).is_stop
      limit: s.open_connections <= s.max_connections
      bad_route: r.invalid_route.is_some implies REST_SERVER.start(args, 8090, r, n).is_stop
      fresh: REST_SERVER.make(8090, r, n).open_connections = 0
      moves: s.moves_to(e, s.serve(e))",
    );
    assert_ok(&Project::new("server", &facts).check());
}

#[test]
fn a_claim_past_the_server_safety_does_not_verify() {
    let lies = [
        // A `Shutdown` stops the server.
        ("claim_runs", "not s.serve(e).is_stop"),
        // Most events send nothing.
        ("claim_answers", "REST_SERVER.commands(s.serve(e)).length = 2"),
        // A server may be full.
        ("claim_room", "s.open_connections < s.max_connections"),
        // An argument that is not a port stops it.
        ("claim_starts", "r.invalid_route.is_none implies not REST_SERVER.start(args, 8090, r, n).is_stop"),
        // A handler may change the state.
        ("claim_frozen", "(match s.serve(e) case Continue(x, _) then x.state = s.state case Stop(_, _) then True end)"),
    ];
    let claims: Vec<String> = lies.iter().map(|(tag, claim)| format!("      {tag}: {claim}")).collect();
    let out = Project::new("server_lies", &server_client(&claims.join("\n"))).check();
    assert!(!out.status.success(), "the lies must fail:\n{}", all(&out));
    for (tag, _) in lies {
        assert!(all(&out).contains(&format!("condition `{tag}` failed")), "{tag} must be refused:\n{}", all(&out));
    }
}

/// A verified service whose routes are `routes`, whose state is a count: `POST /bump` adds
/// one to it, up to 1000, and `GET /count` says it.
fn service(name: &str, routes: &str) -> Project {
    let source = format!(
        "reactive program CLIENT

feature {{ANY}}

  hi(r: REST_REQUEST): HTTP_RESPONSE
    do
      REST_RESPONSE.ok_text(\"hi\")
    end

  bump(n: NAT, r: REST_REQUEST): REST_OUTCOME[NAT]
    do
      if n < 1000 then
        REST_OUTCOME.make(n + 1, REST_RESPONSE.ok_text(\"bumped\"))
      else
        REST_OUTCOME.make(n, REST_RESPONSE.error(HTTP_STATUS.make(409), \"full\"))
      end
    end

  count(n: NAT, r: REST_REQUEST): HTTP_RESPONSE
    do
      if n <= 1000 then REST_RESPONSE.ok_json(JSON.int(I64.from_nat(n))) else REST_RESPONSE.not_found end
    end

  routes: REST_ROUTER[NAT]
    do
      {routes}
    end

  init(args: SEQ[STRING]): STEP[REST_SERVER[NAT]]
    do
      REST_SERVER.start(args, 8090, routes, 0)
    end

  step(s: REST_SERVER[NAT], e: EVENT): STEP[REST_SERVER[NAT]]
    do
      s.serve(e)
    end

end
"
    );
    let project = Project::new(name, &source);
    assert_ok(&project.check());
    project
}

/// A scripted run of `service`, with `args` and the events `events`.
fn run_service(service: &Project, args: &[&str], events: &str) -> Output {
    Command::new(service.dir.join("build/client_bin.exe"))
        .args(args)
        .env("GARDEL_EVENTS", events)
        .output()
        .expect("the service runs")
}

/// A router with a pattern that is not valid, or an argument that is not a port, stops the
/// service before it listens, with exit status 2, and a port it cannot listen on stops it
/// with exit status 1. A full server closes the next connection it is given and answers none
/// of its requests, each request gets one answer, and a connection is freed by its answer.
#[test]
fn a_service_starts_only_when_it_can_and_keeps_its_limit() {
    let bad_route = service("bad_route", "REST_ROUTER.empty.on_get(\"/hi\", hi).on_get(\"/bad/:\", hi)");
    let broken = run_service(&bad_route, &[], "Shutdown");
    assert_eq!(broken.status.code(), Some(2), "{}", all(&broken));
    assert!(all(&broken).contains("grest: route pattern is not valid: /bad/:"), "{}", all(&broken));
    assert!(!all(&broken).contains("net: Listen"), "{}", all(&broken));

    let good = service("good", "REST_ROUTER.empty.on_get(\"/hi\", hi)");
    let port = run_service(&good, &["99999"], "Shutdown");
    assert_eq!(port.status.code(), Some(2), "{}", all(&port));
    assert!(all(&port).contains("grest: not a port: 99999"), "{}", all(&port));
    let taken = run_service(&good, &[], "ListenFailed:8090:98");
    assert_eq!(taken.status.code(), Some(1), "{}", all(&taken));
    assert!(all(&taken).contains("grest: listen failed"), "{}", all(&taken));

    // 65 connections, then a request on each of the last two and two on the first, each
    // asking to close its connection.
    let request = "GET%20/hi%20HTTP/1.1%0D%0AConnection%3A%20close%0D%0A%0D%0A";
    let mut events: Vec<String> = (1..=65).map(|c| format!("Accepted:{c}")).collect();
    events.push(format!("Received:65:{request}"));
    events.push(format!("Received:64:{request}"));
    events.push(format!("Received:1:{request}"));
    events.push(format!("Received:1:{request}"));
    events.push("Accepted:66".to_string());
    events.push(format!("Received:66:{request}"));
    let full = run_service(&good, &["9001"], &events.join(" "));
    assert_eq!(full.status.code(), Some(0), "{}", all(&full));
    let net: Vec<String> = String::from_utf8_lossy(&full.stderr).lines().filter(|l| l.starts_with("net: ")).map(str::to_owned).collect();
    let answer = "HTTP%2F1%2E1%20200%0D%0AContent%2DType%3A%20text%2Fplain%3B%20charset%3Dutf%2D8%0D%0AConnection%3A%20close%0D%0AContent%2DLength%3A%202%0D%0A%0D%0Ahi";
    let expected = [
        "net: Listen 9001".to_string(),
        // The 65th is closed as it arrives, and its request is not answered.
        "net: Close 65".to_string(),
        "net: Close 65".to_string(),
        format!("net: Send 64 {answer}"),
        "net: Close 64".to_string(),
        format!("net: Send 1 {answer}"),
        "net: Close 1".to_string(),
        // A second request on a connection that was answered is not answered.
        "net: Close 1".to_string(),
        // Two connections were freed, so the 66th is served.
        format!("net: Send 66 {answer}"),
        "net: Close 66".to_string(),
    ];
    assert_eq!(net, expected, "{}", all(&full));
}

/// A connection stays open after an answer (F.8): requests in turn and requests sent at once
/// are answered in order, a `HEAD` answer has no body, and the connection closes after a
/// request that asks for it, after an HTTP/1.0 request, after bytes that are not a request,
/// and after its 100th request, whose answer says so. Bytes after the close are not read.
#[test]
fn a_connection_stays_open_until_it_closes_or_reaches_its_cap() {
    let good = service("persistent", "REST_ROUTER.empty.on_get(\"/hi\", hi)");
    let get = "GET%20/hi%20HTTP/1.1%0D%0A%0D%0A";
    let head = "HEAD%20/hi%20HTTP/1.1%0D%0A%0D%0A";
    let close = "GET%20/hi%20HTTP/1.1%0D%0AConnection%3A%20keep-alive%2C%20Close%0D%0A%0D%0A";
    let old = "GET%20/hi%20HTTP/1.0%0D%0A%0D%0A";
    let events = [
        "Accepted:1".to_string(),
        format!("Received:1:{get}"),
        format!("Received:1:{get}"),
        format!("Received:1:{get}{head}{close}{get}"),
        "Accepted:2".to_string(),
        format!("Received:2:{old}"),
        "Accepted:3".to_string(),
        format!("Received:3:{}", get.repeat(101)),
        "Accepted:4".to_string(),
        format!("Received:4:{}", get.repeat(99)),
        format!("Received:4:{}", get.repeat(2)),
        "Accepted:5".to_string(),
        format!("Received:5:{get}%00%00%0D%0A%0D%0A{get}"),
    ];
    let run = run_service(&good, &["9003"], &events.join(" "));
    assert_eq!(run.status.code(), Some(0), "{}", all(&run));
    let net: Vec<String> = String::from_utf8_lossy(&run.stderr).lines().filter(|l| l.starts_with("net: ")).map(str::to_owned).collect();
    let hi = "HTTP%2F1%2E1%20200%0D%0AContent%2DType%3A%20text%2Fplain%3B%20charset%3Dutf%2D8%0D%0AContent%2DLength%3A%202%0D%0A%0D%0Ahi";
    let hi_head = "HTTP%2F1%2E1%20200%0D%0AContent%2DType%3A%20text%2Fplain%3B%20charset%3Dutf%2D8%0D%0AContent%2DLength%3A%202%0D%0A%0D%0A";
    let hi_close = "HTTP%2F1%2E1%20200%0D%0AContent%2DType%3A%20text%2Fplain%3B%20charset%3Dutf%2D8%0D%0AConnection%3A%20close%0D%0AContent%2DLength%3A%202%0D%0A%0D%0Ahi";
    let bad = "HTTP%2F1%2E1%20400%0D%0AConnection%3A%20close%0D%0AContent%2DLength%3A%200%0D%0A%0D%0A";
    let mut expected = vec![
        "net: Listen 9003".to_string(),
        format!("net: Send 1 {hi}"),
        format!("net: Send 1 {hi}"),
        format!("net: Send 1 {hi}"),
        format!("net: Send 1 {hi_head}"),
        format!("net: Send 1 {hi_close}"),
        "net: Close 1".to_string(),
        format!("net: Send 2 {hi_close}"),
        "net: Close 2".to_string(),
    ];
    for conn in [3, 4] {
        expected.extend((1..100).map(|_| format!("net: Send {conn} {hi}")));
        expected.push(format!("net: Send {conn} {hi_close}"));
        expected.push(format!("net: Close {conn}"));
    }
    expected.extend([format!("net: Send 5 {hi}"), format!("net: Send 5 {bad}"), "net: Close 5".to_string()]);
    assert_eq!(net.len(), expected.len(), "{}", net.join("\n"));
    for (i, (got, want)) in net.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "line {i}");
    }
}

/// The sweep (timer 0) closes a connection on the fifth sweep without a completed request,
/// counted from when it was accepted or last answered (F.8). Bytes that do not complete a
/// request do not count as one, and other timers do nothing.
#[test]
fn a_connection_that_completes_no_request_is_swept() {
    let good = service("sweep", "REST_ROUTER.empty.on_get(\"/hi\", hi)");
    let get = "GET%20/hi%20HTTP/1.1%0D%0A%0D%0A";
    let sweeps = |n: usize| vec!["Tick:0"; n].join(" ");
    let events = [
        "Accepted:1".to_string(),
        sweeps(3),
        "Accepted:2".to_string(),
        "Accepted:3".to_string(),
        sweeps(1),
        format!("Received:2:{get}"),
        "Received:3:GET%20".to_string(),
        "Received:2:GET%20/hi".to_string(),
        sweeps(1),
        "Received:3:/hi".to_string(),
        vec!["Tick:7"; 10].join(" "),
        sweeps(1),
        "Received:3:%20HTTP".to_string(),
        sweeps(1),
        "Received:3:/1.1".to_string(),
        sweeps(1),
        "Received:2:%20HTTP/1.1%0D%0A%0D%0A".to_string(),
        sweeps(4),
        "Accepted:4".to_string(),
        sweeps(1),
    ];
    let run = run_service(&good, &["9004"], &events.join(" "));
    assert_eq!(run.status.code(), Some(0), "{}", all(&run));
    let net: Vec<String> = String::from_utf8_lossy(&run.stderr).lines().filter(|l| l.starts_with("net: ")).map(str::to_owned).collect();
    let hi = "HTTP%2F1%2E1%20200%0D%0AContent%2DType%3A%20text%2Fplain%3B%20charset%3Dutf%2D8%0D%0AContent%2DLength%3A%202%0D%0A%0D%0Ahi";
    let expected = [
        "net: Listen 9004".to_string(),
        format!("net: Send 2 {hi}"),
        "net: Close 1".to_string(),
        "net: Close 3".to_string(),
        format!("net: Send 2 {hi}"),
        "net: Close 2".to_string(),
    ];
    assert_eq!(net, expected, "{}", net.join("\n"));
}

/// The state a handler gives back is the one the next request sees, in the order the requests
/// complete, and a request that is not answered by a handler, or that cannot be read, leaves
/// it as it was.
#[test]
fn a_service_keeps_the_state_its_handlers_give() {
    let counting = service(
        "counting",
        "REST_ROUTER.empty.on(Post, \"/bump\", bump).on_view(\"/count\", count)",
    );
    let req = |m: &str, path: &str| format!("{m}%20{path}%20HTTP/1.1%0D%0A%0D%0A");
    let events = [
        "Accepted:1".to_string(),
        "Accepted:2".to_string(),
        "Accepted:3".to_string(),
        "Accepted:4".to_string(),
        "Accepted:5".to_string(),
        "Accepted:6".to_string(),
        // The first part of a request on 1, which completes after 2's.
        "Received:1:POST%20/bump%20HTTP/1.1%0D%0A".to_string(),
        format!("Received:2:{}", req("POST", "/bump")),
        "Received:1:%0D%0A".to_string(),
        // Not a route, a method with no route, a target that cannot be read, and bytes that
        // are not a request: none of them reaches a handler.
        format!("Received:3:{}", req("POST", "/nothing")),
        format!("Received:4:{}", req("DELETE", "/bump")),
        format!("Received:5:{}", req("GET", "/count%25zz")),
        "Received:6:%00%00%0D%0A%0D%0A".to_string(),
        "Accepted:7".to_string(),
        format!("Received:7:{}", req("GET", "/count")),
    ];
    let run = run_service(&counting, &["9002"], &events.join(" "));
    assert_eq!(run.status.code(), Some(0), "{}", all(&run));
    let sends: Vec<String> = String::from_utf8_lossy(&run.stderr)
        .lines()
        .filter(|l| l.starts_with("net: Send "))
        .map(|l| {
            let words: Vec<&str> = l.split_whitespace().collect();
            format!("{} {}", words[2], &words[3]["HTTP%2F1%2E1%20".len()..][..3])
        })
        .collect();
    assert_eq!(sends, ["2 200", "1 200", "3 404", "4 405", "5 400", "6 400", "7 200"], "{}", all(&run));
    let last = String::from_utf8_lossy(&run.stderr).lines().filter(|l| l.starts_with("net: Send 7 ")).collect::<String>();
    assert!(last.ends_with("%0D%0A%0D%0A2"), "two bumps, then the count: {last}");
}

/// A verified service with an async route (F.8): `GET /up` waits through 6 sweeps for an
/// upstream at `127.0.0.1:9999`, whose tag is the state; the upstream is sent `ping` when it
/// connects, and its first bytes answer `pong`; a failed connect answers `502`. `GET /forged`
/// waits with a tag it was not given, and `GET /hi` answers at once.
fn async_service(name: &str) -> Project {
    let source = "reactive program CLIENT

feature {ANY}

  hi(r: REST_REQUEST): HTTP_RESPONSE
    do
      REST_RESPONSE.ok_text(\"hi\")
    end

  up(s: NAT, r: REST_REQUEST, tag: NAT): REST_OUTCOME[NAT]
    do
      REST_OUTCOME.waiting(tag, tag, LIST.empty.prepend(Connect(tag, \"127.0.0.1\", 9999)), 6, REST_RESPONSE.timed_out)
    end

  forged(s: NAT, r: REST_REQUEST): REST_OUTCOME[NAT]
    do
      REST_OUTCOME.waiting(s, 999, LIST.empty.prepend(Connect(999, \"127.0.0.1\", 9999)), 3, REST_RESPONSE.timed_out)
    end

  wake(s: NAT, e: EVENT): REST_WAKE[NAT]
    do
      match e
        case Connected(_, c) then REST_WAKE.make(s, LIST.empty.prepend(Send(c, \"ping\".to_utf8)))
        case ConnectFailed(t, _) then REST_WAKE.answering(s, LIST.empty, t, REST_RESPONSE.error(HTTP_STATUS.make(502), \"down\"))
        case Received(c, _) then REST_WAKE.answering(s, LIST.empty.prepend(Close(c)), s, REST_RESPONSE.ok_text(\"pong\"))
        case _ then REST_WAKE.make(s, LIST.empty)
      end
    end

  routes: REST_ROUTER[NAT]
    do
      REST_ROUTER.empty.on_get(\"/hi\", hi).on_await(Get, \"/up\", up).on(Get, \"/forged\", forged)
    end

  init(args: SEQ[STRING]): STEP[REST_SERVER[NAT]]
    do
      REST_SERVER.start_waking(args, 8090, routes, wake, 0)
    end

  step(s: REST_SERVER[NAT], e: EVENT): STEP[REST_SERVER[NAT]]
    do
      s.serve(e)
    end

end
";
    let project = Project::new(name, source);
    assert_ok(&project.check());
    project
}

/// `s` with its `%XX` escapes decoded.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).expect("an escape"));
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The `net:` lines of a run, with each answer cut to its connection, status and body.
fn answers(run: &Output) -> Vec<String> {
    String::from_utf8_lossy(&run.stderr)
        .lines()
        .filter(|l| l.starts_with("net: "))
        .map(|l| {
            let words: Vec<&str> = l.split_whitespace().collect();
            if words[1] == "Send" && words[3].starts_with("HTTP%2F1%2E1%20") {
                let wire = &words[3];
                let body = decode(wire.rsplit("%0D%0A%0D%0A").next().unwrap_or(""));
                let close = if wire.contains("Connection%3A%20close") { " close" } else { "" };
                format!("Send {} {}{close} {body}", words[2], &wire["HTTP%2F1%2E1%20".len()..][..3])
            } else {
                words[1..].join(" ")
            }
        })
        .collect()
}

/// An async handler (F.8): a request waits while the server answers others, and is answered
/// by its tag when the upstream answers; a request whose upstream does not answer is answered
/// `504` on its sixth sweep, and its connection is not swept as idle meanwhile; a failed
/// connect is a `502`; an answer to a request that no longer waits, as it timed out or its
/// connection closed, is dropped; a handler that waits with a tag it was not given is
/// answered at once; a request after one that waits is not answered, and the connection
/// closes after the answer; bytes while a request waits close the connection, unanswered;
/// and a 33rd request that would wait is a `503`.
#[test]
fn an_async_handler_waits_for_its_answer() {
    let relay = async_service("relay");
    let get = |path: &str| format!("GET%20{path}%20HTTP/1.1%0D%0A%0D%0A");
    let sweeps = |n: usize| vec!["Tick:0"; n].join(" ");
    let mut events = vec![
        // Tag 0: answered by its upstream, while another connection is served.
        "Accepted:1".to_string(),
        format!("Received:1:{}", get("/up")),
        "Accepted:2".to_string(),
        format!("Received:2:{}", get("/hi")),
        "Connected:0:50".to_string(),
        "Received:50:pong".to_string(),
        // The connection reads again.
        format!("Received:1:{}", get("/hi")),
        // Tag 1: no upstream answer, so a 504 on the sixth sweep; then its answer is late.
        format!("Received:2:{}", get("/up")),
        sweeps(3),
        format!("Received:1:{}", get("/hi")),
        sweeps(3),
        "Received:51:late".to_string(),
        // Tag 2: the connect fails.
        format!("Received:1:{}", get("/up")),
        "ConnectFailed:2:10061".to_string(),
        // Tag 3: its connection closes, so the answer is dropped.
        "Accepted:3".to_string(),
        format!("Received:3:{}", get("/up")),
        "Closed:3".to_string(),
        "Received:52:pong".to_string(),
        // A forged tag is answered at once with the late answer, and its Connect is not sent.
        format!("Received:1:{}", get("/forged")),
        // Tag 4: a request after it is not answered, and the connection closes after the answer.
        "Accepted:4".to_string(),
        format!("Received:4:{}{}", get("/up"), get("/hi")),
        "Received:53:pong".to_string(),
        // Tag 5: bytes while it waits close the connection, unanswered.
        "Accepted:5".to_string(),
        format!("Received:5:{}", get("/up")),
        format!("Received:5:{}", get("/hi")),
        "Received:54:pong".to_string(),
    ];
    // Tags 6 to 37: 32 wait, and the 33rd is a 503.
    for c in 10..43 {
        events.push(format!("Accepted:{c}"));
        events.push(format!("Received:{c}:{}", get("/up")));
    }
    let run = run_service(&relay, &["9005"], &events.join(" "));
    assert_eq!(run.status.code(), Some(0), "{}", all(&run));
    let mut expected: Vec<String> = [
        "Listen 9005",
        "Connect 0 127%2E0%2E0%2E1 9999",
        "Send 2 200 hi",
        "Send 50 ping",
        "Close 50",
        "Send 1 200 pong",
        "Send 1 200 hi",
        "Connect 1 127%2E0%2E0%2E1 9999",
        "Send 1 200 hi",
        "Send 2 504 {\"error\":\"timed out\"}",
        "Close 51",
        "Connect 2 127%2E0%2E0%2E1 9999",
        "Send 1 502 {\"error\":\"down\"}",
        "Connect 3 127%2E0%2E0%2E1 9999",
        "Close 52",
        "Send 1 504 {\"error\":\"timed out\"}",
        "Connect 4 127%2E0%2E0%2E1 9999",
        "Close 53",
        "Send 4 200 close pong",
        "Close 4",
        "Connect 5 127%2E0%2E0%2E1 9999",
        "Close 5",
        "Close 54",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    expected.extend((6..38).map(|t| format!("Connect {t} 127%2E0%2E0%2E1 9999")));
    expected.push("Send 42 503 {\"error\":\"busy\"}".to_string());
    let got = answers(&run);
    assert_eq!(got, expected, "{}", got.join("\n"));
}
