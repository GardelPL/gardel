//! A failed obligation is placed where Tablon's Problems view can send the reader (F.11d).
//!
//! An obligation whose tag is in no line, such as an overflow, was placed at the first match of
//! the routine's name in the file, a comment included; a tag shared by two routines at the
//! first routine's; and a broken invariant was reported under the first clause's tag, as each
//! tag asserted the whole invariant. `COUNTER` has the name of `next` in a comment, a call and
//! a tag before its header, and `kept:` in a comment of the routine that fails it.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const COUNTER: &str = "-- A counter: next gives the one after, wrong claims too much, and bump passes the cap.
module COUNTER

representation {NONE}
  n: NAT
  cap: NAT

invariant
  ranged: cap <= 100
  below: n <= cap

create {ANY}

  make: COUNTER
    do
      { n = 0, cap = 10 }
    ensure
      next: Result.size = 0
    end

feature {ANY}

  -- Unlike next and wrong, size keeps its promise.
  size: NAT
    do
      Current.n
    ensure
      kept: Result = Current.n
    end

  after: NAT
    do
      Current.next(Current.n)
    end

  next(x: NAT): NAT
    do
      x + 1
    end

  wrong: NAT
    do
      -- No more than kept: the count itself.
      Current.n
    ensure
      kept: Result > Current.n
    end

  bump: COUNTER
    do
      { Current with n = Current.n + 1 }
    end

end
";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn check(name: &str) -> (Dir, String) {
    let dir = Dir(std::env::temp_dir().join(format!("gardelc_error_place_{}_{name}", std::process::id())));
    drop(fs::remove_dir_all(&dir.0));
    fs::create_dir_all(dir.0.join("src")).expect("temporary directory");
    fs::write(dir.0.join("Gardel.toml"), "[project]\nname = \"counter\"\nversion = \"0.1.0\"\n").expect("manifest");
    fs::write(dir.0.join("src/counter.gdl"), COUNTER).expect("source");
    let out: Output = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir.0)
        .env("GARDEL_CACHE_DIR", dir.0.join("cache"))
        .args(["--project", ".", "--strict"])
        .output()
        .expect("gardelc runs");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(!out.status.success(), "{text}");
    (dir, text)
}

/// The 1-based line of the first line of `COUNTER` that starts, after its indentation, with
/// `start`, and that line's indentation plus one.
fn place(start: &str, nth: usize) -> String {
    let (idx, line) = COUNTER
        .lines()
        .enumerate()
        .filter(|(_, l)| l.trim_start().starts_with(start))
        .nth(nth)
        .expect("the line is in the source");
    format!("src/counter.gdl:{}:{}", idx + 1, line.len() - line.trim_start().len() + 1)
}

/// The diagnostic line of `routine`'s failed `tag`.
fn report<'a>(text: &'a str, routine: &str, tag: &str) -> &'a str {
    let head = format!("in `{routine}` ({tag}) at ");
    text.lines().find(|l| l.contains(&head)).unwrap_or_else(|| panic!("no `{tag}` in `{routine}`:\n{text}"))
}

#[test]
fn an_overflow_is_placed_at_its_routines_header_not_in_a_comment() {
    let (_dir, text) = check("overflow");
    let line = report(&text, "next", "overflow_add");
    assert!(line.contains(&place("next(x: NAT)", 0)), "{line}\n{text}");
}

#[test]
fn a_tag_two_routines_share_is_placed_in_the_routine_that_failed() {
    let (_dir, text) = check("shared");
    let line = report(&text, "wrong", "kept");
    assert!(line.contains(&place("kept:", 1)), "{line}\n{text}");
}

#[test]
fn a_broken_invariant_is_reported_under_its_own_clause() {
    let (_dir, text) = check("invariant");
    let line = report(&text, "bump", "below");
    assert!(line.contains(&place("below:", 0)), "{line}\n{text}");
    assert!(!text.contains("in `bump` (ranged)"), "{text}");
}
