//! A routine whose body is hidden from a module, such as `MAP.get`, is one function at every
//! fuel. Its laws speak of it at the default fuel, and a routine of the module that calls it,
//! `find(id) do Current.items.get(id) end`, unfolds to a call at one fuel less. The laws did
//! not reach that call, so `remove(id).find(id)` was not known to be `None`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const PROGRAM: &str = "module SHELF

representation {NONE}
  items: MAP[NAT, NAT]

create {ANY}

  empty: SHELF
    do
      { items = MAP.empty }
    end

feature {ANY}

  find(id: NAT): OPTION[NAT]
    do
      Current.items.get(id)
    end

  add(id: NAT, v: NAT): SHELF
    do
      { items = Current.items.put(id, v) }
    ensure
      found: Result.find(id).is_some
      others: forall k: NAT. k != id implies (Result.find(k).is_some = Current.find(k).is_some)
    end

  remove(id: NAT): SHELF
    do
      { items = Current.items.remove(id) }
    ensure
      gone: Result.find(id).is_none
CLAIM    end

end
";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn check(name: &str, claim: &str) -> (Dir, Output) {
    let dir = Dir(std::env::temp_dir().join(format!("gardelc_hidden_fuel_{}_{name}", std::process::id())));
    drop(fs::remove_dir_all(&dir.0));
    fs::create_dir_all(&dir.0).expect("temporary directory");
    fs::write(dir.0.join("shelf.gdl"), PROGRAM.replace("CLAIM", claim)).expect("source");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir.0)
        .env("GARDEL_CACHE_DIR", dir.0.join("cache"))
        .args(["shelf.gdl", "--strict"])
        .output()
        .expect("gardelc runs");
    (dir, out)
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn a_law_of_a_hidden_routine_reaches_a_call_in_a_body() {
    let (_dir, out) = check("ok", "");
    let text = all(&out);
    assert!(out.status.success() && text.contains("0 errors, 0 warnings"), "{text}");
}

/// The laws still say only what they say: a remove leaves the other keys.
#[test]
fn a_claim_past_the_laws_is_refused() {
    let (_dir, out) = check("lie", "      wiped: forall k: NAT. Result.find(k).is_none\n");
    assert!(!out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("condition `wiped` failed"), "{}", all(&out));
}
