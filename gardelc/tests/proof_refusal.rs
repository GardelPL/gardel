//! What `gardelc` reports when it has no proof to report: its exit status must say so.
//! These run the compiler as a process, since they change its environment.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// False at one input in 2^64, so sampling does not see it. The verifier refutes it.
const FALSE_AT_ONE_POINT: &str = "functions HIDE

feature {ANY}

  f(x: U64): U64
    do
      if x = 123456789 then 0 else 1 end
    ensure
      lie: Result = 1
    end

end
";

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

/// A directory of its own for `name`, holding one source file.
fn project(name: &str, file: &str, src: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gardelc_refusal_{}_{name}", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(&dir).expect("temporary directory");
    fs::write(dir.join(file), src).expect("source file");
    dir
}

fn gardelc(dir: &PathBuf) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_gardelc"));
    cmd.current_dir(dir).env("GARDEL_CACHE_DIR", dir.join("cache"));
    cmd
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn assert_nothing_proven(out: &Output) {
    assert!(!out.status.success(), "reported success with no proof: {}", text(out));
    assert!(text(out).contains("nothing is proven"), "{}", text(out));
}

#[test]
fn a_verifier_that_cannot_run_is_not_a_proof() {
    let dir = project("broken", "hide.gdl", FALSE_AT_ONE_POINT);
    let out = gardelc(&dir)
        .args(["hide.gdl", "--strict"])
        .env("BOOGIE_EXE", dir.join("no_such_boogie.exe"))
        .output()
        .expect("gardelc runs");
    assert_nothing_proven(&out);
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn a_verifier_that_is_not_installed_is_not_a_proof() {
    for (name, src, file) in
        [("absent_false", FALSE_AT_ONE_POINT, "hide.gdl"), ("absent_true", TRUE, "fine.gdl")]
    {
        let dir = project(name, file, src);
        let out = gardelc(&dir)
            .args([file, "--strict"])
            .env_remove("BOOGIE_EXE")
            .env("USERPROFILE", &dir)
            .env("HOME", &dir)
            .env("PATH", "")
            .output()
            .expect("gardelc runs");
        assert_nothing_proven(&out);
        drop(fs::remove_dir_all(&dir));
    }
}

#[test]
fn bounded_checking_needs_no_verifier_when_it_is_asked_for() {
    let dir = project("bounded", "fine.gdl", TRUE);
    let out = gardelc(&dir)
        .args(["fine.gdl", "--bounded-check"])
        .env("BOOGIE_EXE", dir.join("no_such_boogie.exe"))
        .output()
        .expect("gardelc runs");
    assert!(out.status.success(), "{}", text(&out));
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn a_mistyped_fuel_is_refused() {
    let dir = project("fuel", "fine.gdl", TRUE);
    let out = gardelc(&dir).args(["fine.gdl", "--fuel", "abc"]).output().expect("gardelc runs");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("invalid fuel"), "{}", text(&out));
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn bounded_checking_never_reports_a_proof() {
    let dir = project("bounded_report", "fine.gdl", TRUE);
    let run = |flags: &[&str]| {
        gardelc(&dir).arg("fine.gdl").args(flags).output().expect("gardelc runs")
    };

    let out = run(&["--bounded-check"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("Not Proven"), "{}", text(&out));
    assert!(!text(&out).contains("obligations satisfied"), "{}", text(&out));

    let out = run(&["--bounded-check", "--json-diagnostics"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("not_proven"), "{}", text(&out));

    drop(fs::remove_dir_all(&dir));
}

#[test]
fn strict_refuses_a_module_that_was_only_sampled() {
    for (name, src, file) in
        [("strict_false", FALSE_AT_ONE_POINT, "hide.gdl"), ("strict_true", TRUE, "fine.gdl")]
    {
        let dir = project(name, file, src);
        let out = gardelc(&dir)
            .args([file, "--bounded-check", "--strict"])
            .output()
            .expect("gardelc runs");
        assert!(!out.status.success(), "reported success with no proof: {}", text(&out));
        assert!(text(&out).contains("Not Proven"), "{}", text(&out));
        drop(fs::remove_dir_all(&dir));
    }
}

#[test]
fn inspect_gives_no_interface_for_a_module_that_was_only_sampled() {
    let dir = project("inspect_sampled", "fine.gdl", TRUE);
    let out = gardelc(&dir)
        .args(["fine.gdl", "--inspect", "--bounded-check"])
        .output()
        .expect("gardelc runs");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("functions FINE"), "{}", text(&out));
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn inspect_gives_no_interface_for_a_module_that_did_not_verify() {
    for (name, flags) in [("text", &[][..]), ("json", &["--json-diagnostics"][..])] {
        let dir = project(&format!("inspect_{name}"), "hide.gdl", FALSE_AT_ONE_POINT);
        let out = gardelc(&dir)
            .args(["hide.gdl", "--inspect"])
            .args(flags)
            .output()
            .expect("gardelc runs");
        assert!(!out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("Postcondition Violation"), "{}", text(&out));
        assert!(!text(&out).contains("functions HIDE"), "{}", text(&out));
        drop(fs::remove_dir_all(&dir));
    }
}

#[test]
fn inspect_shows_a_verified_interface() {
    let dir = project("inspect_ok", "fine.gdl", TRUE);
    let out = gardelc(&dir).args(["fine.gdl", "--inspect"]).output().expect("gardelc runs");
    assert!(out.status.success(), "{}", text(&out));
    assert!(String::from_utf8_lossy(&out.stdout).contains("one: (Result = 1)"), "{}", text(&out));
    drop(fs::remove_dir_all(&dir));
}
