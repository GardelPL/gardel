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

  f(req: HTTP_REQUEST, r: REST_ROUTER, v: JSON, e: JSON_ERROR, u: URL_ERROR, s: STRING): BOOL
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
        "      bad_target: not REQUEST_TARGET.parse(req.target).is_ok implies r.dispatch(req).response_status_code = 400
      no_routes: REQUEST_TARGET.parse(req.target).is_ok implies REST_ROUTER.empty.dispatch(req).response_status_code = 404
      bad_body: REST_RESPONSE.bad_body(e).response_status_code = 400
      url: REST_RESPONSE.bad_target(u).response_status_code = 400
      conflict: REST_RESPONSE.error(HTTP_STATUS.make(409), s).response_status_code = 409
      json: REST_RESPONSE.ok_json(v).response_status_code = 200 or REST_RESPONSE.ok_json(v).response_status_code = 500
      created: REST_RESPONSE.created_json(v).response_status_code = 201 or REST_RESPONSE.created_json(v).response_status_code = 500
      gone: REST_RESPONSE.no_content.response_body_length = 0
      routes: r.route_count <= 64",
    );
    assert_ok(&Project::new("statuses", &facts).check());
}

#[test]
fn a_claim_past_the_statuses_does_not_verify() {
    let lies = [
        // A target that cannot be read is a `400`, even with no routes.
        ("no_routes", "      claim: REST_ROUTER.empty.dispatch(req).response_status_code = 404"),
        // A value too large to send is a `500`.
        ("json", "      claim: REST_RESPONSE.ok_json(v).response_status_code = 200"),
        // A route may answer with any status, `400` too.
        ("routed", "      claim: REQUEST_TARGET.parse(req.target).is_ok implies r.dispatch(req).response_status_code != 400"),
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

  f(s: REST_SERVER, e: EVENT, args: SEQ[STRING], r: REST_ROUTER): BOOL
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
/// one answer and closes it, any other event names no connection, a `Shutdown` stops, and a
/// router with a pattern that is not valid does not start.
#[test]
fn a_client_proves_the_server_safety_through_the_contracts() {
    let facts = server_client(
        "      within: REST_SERVER.within(s, e, s.serve(e))
      stops: e.is_shutdown implies s.serve(e).is_stop
      limit: s.open_connections <= s.max_connections
      bad_route: r.invalid_route.is_some implies REST_SERVER.start(args, 8090, r).is_stop
      fresh: REST_SERVER.make(8090, r).open_connections = 0",
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
        ("claim_starts", "r.invalid_route.is_none implies not REST_SERVER.start(args, 8090, r).is_stop"),
    ];
    let claims: Vec<String> = lies.iter().map(|(tag, claim)| format!("      {tag}: {claim}")).collect();
    let out = Project::new("server_lies", &server_client(&claims.join("\n"))).check();
    assert!(!out.status.success(), "the lies must fail:\n{}", all(&out));
    for (tag, _) in lies {
        assert!(all(&out).contains(&format!("condition `{tag}` failed")), "{tag} must be refused:\n{}", all(&out));
    }
}

/// A verified service whose routes are `routes`.
fn service(name: &str, routes: &str) -> Project {
    let source = format!(
        "reactive program CLIENT

feature {{ANY}}

  hi(r: REST_REQUEST): HTTP_RESPONSE
    do
      REST_RESPONSE.ok_text(\"hi\")
    end

  routes: REST_ROUTER
    do
      {routes}
    end

  init(args: SEQ[STRING]): STEP[REST_SERVER]
    do
      REST_SERVER.start(args, 8090, routes)
    end

  step(s: REST_SERVER, e: EVENT): STEP[REST_SERVER]
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

    // 65 connections, then a request on each of the last two and two on the first.
    let request = "GET%20/hi%20HTTP/1.1%0D%0A%0D%0A";
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
    let answer = "HTTP%2F1%2E1%20200%0D%0AContent%2DType%3A%20text%2Fplain%3B%20charset%3Dutf%2D8%0D%0AContent%2DLength%3A%202%0D%0A%0D%0Ahi";
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
