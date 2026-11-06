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
    cmd.current_dir(dir);
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
