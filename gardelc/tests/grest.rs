//! `grest` (Task F.6): the package verifies, its test programs pass (`grest/tests`), and a client
//! proves the limits of a request target and a route match, and the statuses a router and the
//! responses give, through the contracts alone, while claims past them are refused.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

/// The package and its test package verify, and its test programs pass: every case of
/// `cases.txt` (decoding, targets, patterns, matches and answers) and every script of
/// `grest_server_test.events` (a server over events: how it starts, its limit, the connections
/// it keeps and closes, its sweep, its state and the requests that wait).
#[test]
fn grest_verifies_and_passes_its_tests() {
    let cache = std::env::temp_dir().join(format!("gardelc_grest_{}_cache", std::process::id()));
    drop(fs::remove_dir_all(&cache));
    assert_ok(&check_at(&root().join("grest"), &cache));
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(root())
        .env("GARDEL_CACHE_DIR", &cache)
        .args(["test", "grest"])
        .output()
        .expect("gardelc runs");
    assert!(out.status.success(), "{}", all(&out));
    for name in ["GREST_TEST", "GREST_SERVER_TEST"] {
        assert!(all(&out).contains(&format!("[test] {name} ... ok")), "{}", all(&out));
    }
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
