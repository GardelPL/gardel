//! Names that Boogie reserves and Gardel does not (`old`, `var`, `type`, `function`, `int`):
//! a parameter, a `let`, a `match` binder, a lambda parameter and a quantified variable may be
//! called so. Each was a parse error in the emitted Boogie, and the module was `E0199`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const PROGRAM: &str = "program KW

feature {ANY}

  pick(old: OPTION[NAT], type: NAT): NAT
    require
      small: type < 100
    do
      let var = type + 1
      match old
        case Some(old) then if old < 100 then old + var else var end
        case None then var
      end
    ensure
      bounded: Result <= 200
      none: old.is_none implies Result = type + 1
CLAIM    end

  twice(function: (x: NAT) -> NAT, n: NAT): NAT
    require
      small: n < 10
    do
      function(n)
    end

  size(xs: LIST[NAT]): NAT
    do
      xs.length
    ensure
      all: forall int: NAT. int < Result implies int < xs.length
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(NUMBER_TEXT.nat_to_text(pick(Some(5), 7)))
        IO.print_line(NUMBER_TEXT.nat_to_text(pick(None, 7)))
        IO.print_line(NUMBER_TEXT.nat_to_text(twice(fn(old: NAT): NAT do if old < 100 then old + 1 else old end end, 3)))
        IO.exit(0)
      end
    end

end
";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn run(name: &str, claim: &str) -> (Dir, Output) {
    let dir = Dir(std::env::temp_dir().join(format!("gardelc_reserved_{}_{name}", std::process::id())));
    drop(fs::remove_dir_all(&dir.0));
    fs::create_dir_all(&dir.0).expect("temporary directory");
    fs::write(dir.0.join("kw.gdl"), PROGRAM.replace("CLAIM", claim)).expect("source");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir.0)
        .env("GARDEL_CACHE_DIR", dir.0.join("cache"))
        .args(["kw.gdl", "--strict", "--run"])
        .output()
        .expect("gardelc runs");
    (dir, out)
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn names_boogie_reserves_verify_and_run() {
    let (_dir, out) = run("ok", "");
    let text = all(&out);
    assert!(out.status.success() && text.contains("0 errors, 0 warnings"), "{text}");
    let printed: Vec<&str> = text.lines().filter(|l| l.trim().parse::<u64>().is_ok()).map(str::trim).collect();
    assert_eq!(printed, ["13", "8", "4"], "{text}");
}

/// The renamed parameters still mean what they meant: a false claim about them is refused.
#[test]
fn a_claim_about_a_renamed_name_is_refused() {
    let (_dir, out) = run("lie", "      claim: old.is_none implies Result = type\n");
    assert!(!out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("condition `claim` failed"), "{}", all(&out));
}
