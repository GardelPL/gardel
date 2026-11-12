//! A function's `require` guards its definition one fuel below the application (Task F.13).
//! When the application is itself one fuel below its caller, that guard is read at `ZeroFuel`,
//! and a function it calls, such as `size(a)` or `room`, was known nowhere there, so the solver
//! could not unfold the function one step further. The fuel step now goes from each fuel to the
//! one below it, down to `ZeroFuel`, which ties `room` there to `room` where the caller knows it.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// A recursive function whose `require` calls a function with a body, and a lemma that
/// unfolds it one step: `same(a, b, i)` from `same(a, b, i + 1)`.
const SCAN: &str = "module SCAN

create {ANY}

  size(a: BYTES): NAT
    do
      a.length
    end

  same(a: BYTES, b: BYTES, i: NAT): BOOL
    require
      in_bounds: i <= SCAN.size(a)
    variant
      a.length - i
    do
      if a.length != b.length then
        False
      else if i = a.length then
        True
      else if a.at(i) = b.at(i) then
        SCAN.same(a, b, i + 1)
      else
        False
      end end end
    end

  same_equal(a: BYTES, b: BYTES, i: NAT): BOOL
    require
      in_bounds: i <= SCAN.size(a)
EQUAL    variant
      a.length - i
    do
      if i = a.length then
        True
      else
        SCAN.same_equal(a, b, i + 1)
      end
    ensure
      holds: Result
      same: SCAN.same(a, b, i)
    end

end
";

/// A client whose function unfolds a call one fuel below its own, `b.add(x)`, guarded by `room`.
const BAG: &str = "module BAG

representation {NONE}
  items: LIST[NAT]
  count: NAT

create {ANY}

  empty: BAG
    do
      { items = LIST.empty, count = 0 }
    end

feature {ANY}

  room: BOOL
    do
      Current.count < 10
    end

  add(x: NAT): BAG
    require
      has_room: Current.room
    do
      { items = Current.items.prepend(x), count = Current.count + 1 }
    end

  first: NAT
    do
      if Current.items.length = 0 then 0 else Current.items.head end
    end

end
";

const PUT: &str = "functions PUT

feature {ANY}

  put(b: BAG, x: NAT): BAG
    do
      if b.room then b.add(x) else b end
    end

specify {ANY}

  law put_first(b: BAG, x: NAT)
ROOM    ensure
      first: PUT.put(b, x).first = x
    end

end
";

struct Project(PathBuf);

impl Project {
    fn new(name: &str, sources: &[(&str, String)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_fuel_guard_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        let manifest = format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", name.replace('_', "-"));
        fs::write(dir.join("Gardel.toml"), manifest).expect("manifest");
        for (file, text) in sources {
            fs::write(dir.join("src").join(file), text).expect("source file");
        }
        Self(dir)
    }

    fn check(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&self.0)
            .env("GARDEL_CACHE_DIR", self.0.join("cache"))
            .args(["--project", ".", "--strict"])
            .output()
            .expect("gardelc runs")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn assert_ok(out: &Output) {
    assert!(out.status.success() && all(out).contains("0 errors, 0 warnings"), "{}", all(out));
}

fn assert_refused(out: &Output, tag: &str) {
    assert!(!out.status.success(), "{}", all(out));
    assert!(all(out).contains(&format!("condition `{tag}` failed")), "{}", all(out));
}

#[test]
fn a_recursive_function_with_a_require_unfolds_one_step_further() {
    let p = Project::new("scan", &[("scan.gdl", SCAN.replace("EQUAL", "      equal: a.model = b.model\n"))]);
    assert_ok(&p.check());
}

/// Without the hypothesis the claim is false, and it is still refused.
#[test]
fn a_false_claim_about_the_unfolding_is_refused() {
    let p = Project::new("scan_lie", &[("scan.gdl", SCAN.replace("EQUAL", ""))]);
    assert_refused(&p.check(), "same");
}

#[test]
fn a_client_unfolds_a_call_one_fuel_below_its_own() {
    let p = Project::new(
        "put",
        &[("bag.gdl", BAG.to_string()), ("put.gdl", PUT.replace("ROOM", "    require\n      has_room: b.room\n"))],
    );
    assert_ok(&p.check());
}

/// A full bag is left as it is, so without `room` the claim is false, and it is refused.
#[test]
fn a_client_claim_outside_the_require_is_refused() {
    let p = Project::new("put_lie", &[("bag.gdl", BAG.to_string()), ("put.gdl", PUT.replace("ROOM", ""))]);
    assert_refused(&p.check(), "first");
}
