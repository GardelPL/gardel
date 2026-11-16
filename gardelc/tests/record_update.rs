//! A record update of a record update stays small in Boogie. `{ x with f = v }` reads every field
//! of `x` that it keeps, and a `let` is substituted before emission, so a chain of `let`s, each
//! updating the last, copied the base once per field at every step: 30 fields and four `let`s
//! made a 459 MB Boogie file, and Tablon's 53-field state passed the solver's backstop. The
//! update now binds a base that is not a variable once, and reads it through that binding in
//! the values too, so the file grows with the chain, not with its fields to its length.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, files: &[(&str, String)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_record_update_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        fs::write(dir.join("Gardel.toml"), format!("[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n")).expect("manifest");
        for (file, src) in files {
            fs::write(dir.join("src").join(file), src).expect("source file");
        }
        Self { dir }
    }

    fn check(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&self.dir)
            .env("GARDEL_CACHE_DIR", self.dir.join("cache"))
            .args(["--project", ".", "--strict"])
            .output()
            .expect("gardelc runs")
    }

    fn bpl_size(&self, stem: &str) -> u64 {
        fs::metadata(self.dir.join("build").join(format!("{stem}.bpl"))).expect("the Boogie file").len()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// Thirty fields, and three `let`s that each update the last and read it: each step keeps 29
/// fields of its base. `claim` is added to `stepped`'s postconditions.
fn wide(claim: &str) -> String {
    let fields = (0..30).fold(String::new(), |mut out, i| {
        let _ = writeln!(out, "  f{i}: NAT");
        out
    });
    let zero: Vec<String> = (0..30).map(|i| format!("f{i} = 0")).collect();
    format!(
        "-- Thirty fields.
module WIDE

representation {{ANY}}
{fields}
create {{ANY}}

  zero: WIDE
    do
      {{ {} }}
    end

feature {{ANY}}

  stepped(k: NAT): WIDE
    require
      small: k < 1000
    do
      let a = {{ Current with f0 = k }}
      let b = {{ a with f1 = a.f0 + 1 }}
      let c = {{ b with f2 = if b.f1 > 5 then b.f1 + 1 else b.f1 + 1 end }}
      {{ c with f3 = c.f2 + 1 }}
    ensure
      three: Result.f3 = k + 3
      kept: Result.f29 = Current.f29
{claim}
    end

end
",
        zero.join(", ")
    )
}

/// Two fields, both updated at each of eight steps from two reads of the last: the values
/// alone would copy the base four times a step.
fn pair(claim: &str) -> String {
    let mut lets = String::new();
    let mut last = "Current".to_string();
    for i in 0..8 {
        let v = format!("p{i}");
        let _ = writeln!(lets, "      let {v} = {{ {last} with x = {last}.x + {last}.y, y = {last}.y + {last}.x }}");
        last = v;
    }
    format!(
        "-- Two fields, each step adding them.
module PAIR

representation {{ANY}}
  x: NAT
  y: NAT

create {{ANY}}

  of(a: NAT, b: NAT): PAIR
    do
      {{ x = a, y = b }}
    end

feature {{ANY}}

  doubled: PAIR
    require
      small: Current.x < 1000 and Current.y < 1000
    do
{lets}      {last}
    ensure
      same: Result.x = Result.y
      grown: Result.x = 128 * (Current.x + Current.y)
{claim}
    end

end
"
    )
}

/// A field of type `BYTES` read through a shared base: `.length` is `BYTES`'s only if the
/// binding has the module's type, as it is a list's for a name of no known type.
fn row(claim: &str) -> String {
    format!(
        "-- Bytes and a count.
module ROW

representation {{ANY}}
  data: BYTES
  n: NAT
  m: NAT

create {{ANY}}

  of(b: BYTES): ROW
    do
      {{ data = b, n = 0, m = 0 }}
    end

feature {{ANY}}

  counted: ROW
    do
      let a = {{ Current with n = 1 }}
      let b = {{ a with n = a.data.length }}
      {{ b with m = b.data.length }}
    ensure
      both: Result.n = Current.data.length and Result.m = Current.data.length
{claim}
    end

end
"
    )
}

#[test]
fn a_chain_of_record_updates_through_lets_verifies_in_a_small_boogie_file() {
    let p = Project::new("small", &[("wide.gdl", wide("")), ("pair.gdl", pair("")), ("row.gdl", row(""))]);
    let out = p.check();
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
    let (w, q) = (p.bpl_size("wide"), p.bpl_size("pair"));
    assert!(w < 200_000, "wide.bpl has {w} bytes");
    assert!(q < 200_000, "pair.bpl has {q} bytes");
}

#[test]
fn a_lie_about_a_shared_record_update_is_refused() {
    let p = Project::new(
        "lie",
        &[
            ("wide.gdl", wide("      lie: Result.f3 = k + 4")),
            ("pair.gdl", pair("      off: Result.x = Current.x + Current.y")),
        ],
    );
    let out = p.check();
    assert!(!out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("condition `lie` failed"), "{}", all(&out));
    assert!(all(&out).contains("condition `off` failed"), "{}", all(&out));
}
