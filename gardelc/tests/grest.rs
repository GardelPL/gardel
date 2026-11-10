//! `grest` (Task F.6): the package verifies, every case of `grest/tests/cases.txt` holds in C,
//! and a client proves the limits of a request target and a route match through the contracts
//! alone, while claims past those limits are refused.

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
    /// A project in a directory of its own that depends on `grest`.
    fn new(name: &str, source: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_grest_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        let path = root().join("grest").canonicalize().expect("grest exists");
        let path = path.to_string_lossy().replace('\\', "/").replace("//?/", "");
        let manifest = format!(
            "[project]\nname = \"{}\"\nversion = \"0.1.0\"\n\n[dependencies]\ngrest = {{ path = \"{path}\" }}\n",
            name.replace('_', "-")
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
