//! The local verification cache through the command line: it may save the solver's work, and
//! must never change what a run reports. Each test has a cache of its own.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TRUE: &str = "functions FINE

feature {ANY}

  f(x: U64): U64
    do
      1
    ensure
      one: Result = 1
    end

end
";

/// Finding 1 of the Roadmap. Its contract is false, and a client would believe it.
const FALSE_LIB: &str = "functions LIB

feature {ANY}

  f(x: U64): U64
    do
      1
    ensure
      lie: Result = 2
    end

end
";

const CLIENT: &str = "functions CLIENT

feature {ANY}

  g(x: U64): U64
    do
      LIB.f(x)
    ensure
      believes: Result = 2
    end

end
";

const FORGED_INTERFACE: &str = "functions LIB

feature {ANY}

  f(x: U64): U64
    ensure
      lie: Result = 2
    end

end
";

/// Four bounds in a map: a run that emits them in the map's order differs nearly every time.
const BOUNDED: &str = "module PACK[A: EQ, B: EQ, C: EQ, D: EQ]

representation {ANY}
  | Pack(a: A, b: B, c: C, d: D)

feature {ANY}

  first: A
    do
      match Current
      case Pack(a, b, c, d) then a
      end
    end

end
";

const ASSUMES: &str = "functions HOLE

feature {ANY}

  f(x: U64): U64
    do
      let y = if x > 0 then
          assume hole: False end
          1
        else 7 end
      y
    ensure
      lie: Result = 7
    end

end
";

const FIRST: &str = "functions FIRST

feature {ANY}

  f(l: LIST[U64], c: U64): LIST[U64]
    do
      LIST_OPS.filter(l, fn(x: U64): BOOL do x != c end)
    end

end
";

const SECOND: &str = "functions SECOND

feature {ANY}

  g(l: LIST[U64], c: U64): LIST[U64]
    do
      LIST_OPS.filter(l, fn(x: U64): BOOL do x != c end)
    end

end
";

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_cache_cli_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).expect("temporary directory");
        for (file, src) in files {
            fs::write(dir.join(file), src).expect("source file");
        }
        Self { dir }
    }

    fn cache(&self) -> PathBuf {
        self.dir.join("cache")
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_gardelc"));
        cmd.current_dir(&self.dir).env("GARDEL_CACHE_DIR", self.cache());
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("gardelc runs")
    }

    fn entries(&self) -> Vec<PathBuf> {
        let Ok(dir) = fs::read_dir(self.cache()) else {
            return Vec::new();
        };
        dir.map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "proof"))
            .collect()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// (modules verified now, modules from the cache).
fn tally(out: &Output) -> (usize, usize) {
    let text = text(out);
    let line = text.lines().find_map(|l| l.trim().strip_prefix("[cache] ")).unwrap_or_else(|| panic!("{text}"));
    let mut numbers = line.split_whitespace().filter_map(|w| w.parse().ok());
    (numbers.next().expect("verified"), numbers.next().expect("cached"))
}

fn assert_refuted(out: &Output, tag: &str) {
    assert!(!out.status.success(), "{}", text(out));
    assert!(text(out).contains(tag), "{}", text(out));
}

#[test]
fn a_second_run_needs_no_solver_and_reports_the_same() {
    let p = Project::new("second", &[("fine.gdl", TRUE)]);
    let first = p.run(&["fine.gdl", "--strict"]);
    assert!(first.status.success(), "{}", text(&first));
    assert_eq!(tally(&first), (1, 0));
    let second = p.run(&["fine.gdl", "--strict"]);
    assert!(second.status.success(), "{}", text(&second));
    assert_eq!(tally(&second), (0, 1));
    assert!(text(&second).contains("0 errors, 0 warnings"), "{}", text(&second));
}

#[test]
fn the_key_is_the_same_in_every_process() {
    let p = Project::new("stable", &[("pack.gdl", BOUNDED)]);
    let first = p.run(&["pack.gdl", "--strict"]);
    assert!(first.status.success(), "{}", text(&first));
    for _ in 0..3 {
        assert_eq!(tally(&p.run(&["pack.gdl", "--strict"])), (0, 1));
    }
}

#[test]
fn a_warm_run_finds_what_a_cold_run_stored() {
    // A cold run emits each module twice, and a warm one once: a module's text must not depend
    // on that, or the second module here would miss.
    let p = Project::new("order", &[("first.gdl", FIRST), ("second.gdl", SECOND)]);
    let args = ["first.gdl", "second.gdl", "--strict"];
    assert_eq!(tally(&p.run(&args)), (2, 0));
    assert_eq!(tally(&p.run(&args)), (0, 2));
}

#[test]
fn a_changed_source_is_verified_again() {
    let p = Project::new("changed", &[("fine.gdl", TRUE)]);
    assert_eq!(tally(&p.run(&["fine.gdl"])), (1, 0));
    fs::write(p.dir.join("fine.gdl"), TRUE.replace("      1\n", "      2\n")).expect("source file");
    let out = p.run(&["fine.gdl"]);
    assert_eq!(tally(&out), (1, 0));
    assert_refuted(&out, "one");
}

#[test]
fn a_changed_limit_or_seed_is_verified_again() {
    let p = Project::new("limits", &[("fine.gdl", TRUE)]);
    assert_eq!(tally(&p.run(&["fine.gdl"])), (1, 0));
    assert_eq!(tally(&p.run(&["fine.gdl", "--z3-rlimit", "20000000"])), (1, 0));
    assert_eq!(tally(&p.run(&["fine.gdl", "--z3-seed", "7"])), (1, 0));
    assert_eq!(tally(&p.run(&["fine.gdl", "--fuel", "3"])), (1, 0));
    assert_eq!(tally(&p.run(&["fine.gdl"])), (0, 1));
}

#[test]
fn a_failure_is_never_stored() {
    let p = Project::new("failure", &[("lib.gdl", FALSE_LIB)]);
    for _ in 0..2 {
        let out = p.run(&["lib.gdl"]);
        assert_eq!(tally(&out), (1, 0));
        assert_refuted(&out, "lie");
    }
    assert!(p.entries().is_empty());
}

#[test]
fn a_mutated_entry_is_a_miss() {
    let p = Project::new("mutated", &[("fine.gdl", TRUE)]);
    assert_eq!(tally(&p.run(&["fine.gdl"])), (1, 0));
    let entries = p.entries();
    assert_eq!(entries.len(), 1);
    let mut bytes = fs::read(&entries[0]).expect("entry");
    let last_digit = bytes.len() - 2;
    bytes[last_digit] ^= 1;
    fs::write(&entries[0], &bytes).expect("mutated entry");
    let out = p.run(&["fine.gdl"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(tally(&out), (1, 0));
    // The run replaced the entry it could not use.
    assert_eq!(tally(&p.run(&["fine.gdl"])), (0, 1));
}

#[test]
fn a_cache_from_elsewhere_proves_nothing() {
    let p = Project::new("elsewhere", &[("fine.gdl", TRUE)]);
    assert_eq!(tally(&p.run(&["fine.gdl"])), (1, 0));
    // The entries arrive, say with a package, on a machine with its own secret.
    fs::remove_file(p.cache().join("machine.secret")).expect("secret");
    assert_eq!(tally(&p.run(&["fine.gdl"])), (1, 0));
}

#[test]
fn a_false_interface_file_is_never_read() {
    let p = Project::new(
        "interface",
        &[("lib.gdl", FALSE_LIB), ("client.gdl", CLIENT), ("lib.gds", FORGED_INTERFACE)],
    );
    assert_refuted(&p.run(&["lib.gdl", "client.gdl", "--strict"]), "lie");
}

#[test]
fn strict_is_not_answered_by_a_proof_from_a_weaker_run() {
    let p = Project::new("strict", &[("hole.gdl", ASSUMES)]);
    let weak = p.run(&["hole.gdl"]);
    assert!(weak.status.success(), "{}", text(&weak));
    let strict = p.run(&["hole.gdl", "--strict"]);
    assert_eq!(tally(&strict), (0, 1), "{}", text(&strict));
    assert_refuted(&strict, "--strict Assume Disallowed");
}

#[test]
fn the_entry_records_the_assumes_it_rests_on() {
    let p = Project::new("recorded", &[("hole.gdl", ASSUMES)]);
    assert!(p.run(&["hole.gdl"]).status.success());
    let entries = p.entries();
    assert_eq!(entries.len(), 1);
    let entry = fs::read_to_string(&entries[0]).expect("entry");
    assert!(entry.contains("\nmodule HOLE\n"), "{entry}");
    assert!(entry.contains("\nassume f hole\n"), "{entry}");
}

#[test]
fn verify_deps_ignores_the_cache_and_refreshes_it() {
    let p = Project::new("verify_deps", &[("fine.gdl", TRUE)]);
    assert_eq!(tally(&p.run(&["fine.gdl"])), (1, 0));
    assert_eq!(tally(&p.run(&["fine.gdl", "--verify-deps"])), (1, 0));
    assert_eq!(tally(&p.run(&["fine.gdl", "--verify-deps"])), (1, 0));
    assert_eq!(tally(&p.run(&["fine.gdl"])), (0, 1));
}

#[test]
fn a_warm_cache_does_not_excuse_a_verifier_that_cannot_run() {
    let p = Project::new("broken", &[("fine.gdl", TRUE)]);
    assert_eq!(tally(&p.run(&["fine.gdl"])), (1, 0));
    let out = p
        .command()
        .args(["fine.gdl", "--strict"])
        .env("BOOGIE_EXE", p.dir.join("no_such_boogie.exe"))
        .output()
        .expect("gardelc runs");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("nothing is proven"), "{}", text(&out));
}

#[test]
fn sampling_neither_reads_the_cache_nor_adds_to_it() {
    let p = Project::new("sampling", &[("fine.gdl", TRUE)]);
    assert!(p.run(&["fine.gdl", "--bounded-check"]).status.success());
    assert!(p.entries().is_empty());
    assert_eq!(tally(&p.run(&["fine.gdl"])), (1, 0));
    let sampled = p.run(&["fine.gdl", "--bounded-check"]);
    assert!(text(&sampled).contains("Not Proven"), "{}", text(&sampled));
    assert!(!text(&sampled).contains("[cache]"), "{}", text(&sampled));
}

#[test]
fn json_output_stays_a_json_document() {
    let p = Project::new("json", &[("fine.gdl", TRUE)]);
    let out = p.run(&["fine.gdl", "--json-diagnostics"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim().replace(['\r', '\n'], ""), "[]");
    assert_eq!(tally(&out), (1, 0));
}

#[test]
fn a_cache_that_cannot_be_used_only_costs_time() {
    let p = Project::new("unusable", &[("fine.gdl", TRUE)]);
    let not_a_directory: &Path = &p.dir.join("fine.gdl");
    let out = p
        .command()
        .arg("fine.gdl")
        .env("GARDEL_CACHE_DIR", not_a_directory)
        .output()
        .expect("gardelc runs");
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("[cache] off"), "{}", text(&out));
}
