//! `match` binders of the same name and different types in one routine (Task F.6). The
//! verifier declares each binder as a local of the procedure, so `case Some(e)` beside
//! `case Error(e)` gave one local two types and the module was `E0199`. A repeated binder is
//! now renamed in its arm: the contracts that use it verify, a lie about it is refused, and
//! the program computes the same values in C.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, src: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_binders_{}_{name}", std::process::id()));
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

/// `e` is a `NAT` in one arm and a `STRING_ERROR` in another, `n` is a parameter and then a
/// binder, and `x` is a `let` and then a binder.
fn program(claim: &str) -> String {
    format!(
        "program MAIN

feature {{ANY}}

  pick(a: OPTION[NAT], b: RESULT[STRING_ERROR, NAT], n: NAT): NAT
    require
      small: n < 1000
    do
      let x = n + 1
      match a
        case None then
          match b
            case Error(e) then x
            case Ok(n) then n
          end
        case Some(e) then
          match a
            case Some(x) then if e < x then x else e end
            case None then 0
          end
      end
    ensure
      some: a.is_some implies Result = a.get
      ok: (not a.is_some and b.is_ok) implies Result = b.get
      error: (not a.is_some and not b.is_ok) implies Result = n + 1
{claim}
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let bad: RESULT[STRING_ERROR, NAT] = Error(OverlongEncoding(0))
      let good: RESULT[STRING_ERROR, NAT] = Ok(30)
      -- Typed values: a constructor as the argument is not yet typed in the caller's
      -- precondition check.
      let seven: OPTION[NAT] = Some(7)
      let none: OPTION[NAT] = None
      with IO
        IO.print_line(NUMBER_TEXT.nat_to_text(pick(seven, good, 1)))
        IO.print_line(NUMBER_TEXT.nat_to_text(pick(none, good, 1)))
        IO.print_line(NUMBER_TEXT.nat_to_text(pick(none, bad, 41)))
        IO.exit(0)
      end
    end

end
"
    )
}

#[test]
fn binders_of_one_name_and_two_types_verify_and_run() {
    let p = Project::new("binders", &program(""));
    let out = p.check();
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
    let run = Command::new(p.dir.join("build").join("main_bin.exe")).output().expect("the program runs");
    assert!(run.status.success(), "{run:?}");
    let printed = String::from_utf8(run.stdout).expect("UTF-8 output").replace("\r\n", "\n");
    assert_eq!(printed.lines().collect::<Vec<_>>().join(" "), "7 30 42");
}

#[test]
fn a_lie_about_a_renamed_binder_is_refused() {
    let p = Project::new("binders-lie", &program("      claim: (not a.is_some and b.is_ok) implies Result = n"));
    let out = p.check();
    assert!(!out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("condition `claim` failed"), "{}", all(&out));
}
