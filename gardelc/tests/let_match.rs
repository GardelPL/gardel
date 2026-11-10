//! A `let` bound to a `match` expression (Task F.6). The verifier gave the local the type
//! `int` whatever its arms gave, so `let s = match a case Some(x) then x case None then "" end`
//! made the module `E0199`. The local now has the type of its arms: the contracts that use it
//! verify, a lie about it is refused, and the program computes the same values in C.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, src: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_let_match_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        fs::write(dir.join("Gardel.toml"), format!("[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n")).expect("manifest");
        fs::write(dir.join("src").join("main.gdl"), src).expect("source file");
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
}

impl Drop for Project {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// A `STRING` from a binder or a literal, and an `OPTION[NAT]` from a constructor in each arm.
fn program(claim: &str) -> String {
    format!(
        "program MAIN

feature {{ANY}}

  name_or(a: OPTION[STRING]): STRING
    do
      let s = match a case Some(x) then x case None then \"none\" end
      s
    ensure
      some: a.is_some implies Result.equals(a.get)
{claim}
    end

  same(a: OPTION[NAT]): OPTION[NAT]
    do
      let o = match a case Some(x) then Some(x + 0) case None then None end
      o
    ensure
      kept: a.is_some implies (Result.is_some and Result.get = a.get)
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let none: OPTION[STRING] = None
      let ana: OPTION[STRING] = Some(\"ana\")
      let seven: OPTION[NAT] = Some(7)
      with IO
        IO.print_line(name_or(none))
        IO.print_line(name_or(ana))
        IO.print_line(NUMBER_TEXT.nat_to_text(same(seven).get))
        IO.exit(0)
      end
    end

end
"
    )
}

#[test]
fn a_let_bound_to_a_match_has_the_type_of_its_arms() {
    let p = Project::new("let-match", &program(""));
    let out = p.check();
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
    let run = Command::new(p.dir.join("build").join("main_bin.exe")).output().expect("the program runs");
    assert!(run.status.success(), "{run:?}");
    let printed = String::from_utf8(run.stdout).expect("UTF-8 output").replace("\r\n", "\n");
    assert_eq!(printed.lines().collect::<Vec<_>>().join(" "), "none ana 7");
}

#[test]
fn a_lie_about_a_let_bound_to_a_match_is_refused() {
    let p = Project::new("let-match-lie", &program("      claim: Result.byte_length > 0 implies a.is_some"));
    let out = p.check();
    assert!(!out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("condition `claim` failed"), "{}", all(&out));
}
