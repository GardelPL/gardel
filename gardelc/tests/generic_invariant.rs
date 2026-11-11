//! Every value of a generic module satisfies its invariant, in another module's proofs too
//! (F.8). A module's contracts are guarded by its invariant, and only a parameter assumed it,
//! so a field of type `BAG[T]` read in another module had none of `BAG`'s contracts. This
//! kept `REST_SERVER[S]` from using `REST_ROUTER[S].run`'s.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const BAG: &str = "module BAG[T]

representation {NONE}
  items: LIST[T]
  n: NAT

invariant
  counted: n = items.length
  small: n <= 10

create {ANY}

  empty: BAG[T]
    do
      { items = LIST.empty, n = 0 }
    ensure
      none: Result.size = 0
    end

feature {ANY}

  size: NAT
    do
      Current.n
    ensure
      at_most_ten: Result <= 10
    end

end
";

const SHELF: &str = "module SHELF[T]

representation {NONE}
  bag: BAG[T]
  label: NAT

create {ANY}

  make(b: BAG[T]): SHELF[T]
    do
      { bag = b, label = 0 }
    end

feature {ANY}

  -- The bag is a field, not a parameter: only `BAG`'s invariant gives its contract here.
  room_left: NAT
    do
      10 - Current.bag.size
    ensure
      CLAIM
    end

end
";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn check(name: &str, claim: &str) -> (Dir, Output) {
    let dir = Dir(std::env::temp_dir().join(format!("gardelc_generic_invariant_{}_{name}", std::process::id())));
    drop(fs::remove_dir_all(&dir.0));
    fs::create_dir_all(dir.0.join("src")).expect("temporary directory");
    fs::write(dir.0.join("Gardel.toml"), "[project]\nname = \"shelves\"\nversion = \"0.1.0\"\n").expect("manifest");
    fs::write(dir.0.join("src/bag.gdl"), BAG).expect("source");
    fs::write(dir.0.join("src/shelf.gdl"), SHELF.replace("CLAIM", claim)).expect("source");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir.0)
        .env("GARDEL_CACHE_DIR", dir.0.join("cache"))
        .args(["--project", ".", "--strict"])
        .output()
        .expect("gardelc runs");
    (dir, out)
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// `10 - size` cannot underflow, and the claim holds, from `BAG.size`'s `ensure`.
#[test]
fn a_field_of_a_generic_module_has_its_contracts() {
    let (_dir, out) = check("ok", "bounded: Result <= 10");
    let text = all(&out);
    assert!(out.status.success() && text.contains("0 errors, 0 warnings"), "{text}");
}

/// The invariant gives what it says, no more.
#[test]
fn a_claim_past_the_invariant_is_refused() {
    let (_dir, out) = check("lie", "lie: Result >= 1");
    assert!(!out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("condition `lie` failed"), "{}", all(&out));
}
