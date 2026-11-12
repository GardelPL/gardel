//! Facts over a type at the value of a call (Task F.15). Boogie, under `/typeEncoding:p`,
//! states the type of a function's result with an axiom that guards its arguments' types, and
//! after an `int` or `bool` argument the guards are shifted by one. A call such as
//! `twice(n, b)`, with a `NAT` before the `BOX`, then had no type, and no fact quantified over
//! its type applied to it: neither a law of another module, nor the `LIST` facts at
//! `digits(n, ys).append(zs)`. A `let` gave the value a type, so it worked around the gap in a
//! routine, but a law has no `let`. `boogie_params` now moves the scalars of every function
//! last, as it did for generic ones.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const BOX: &str = "module BOX

representation {NONE}
  v: NAT

invariant
  small: Current.v <= 100

create {ANY}

  make(n: NAT): BOX
    require
      ok: n <= 100
    do
      { v = n }
    end

feature {ANY}

  value: NAT
    do
      Current.v
    end

  bump: BOX
    do
      if Current.v < 100 then { v = Current.v + 1 } else Current end
    end

end
";

const BOX_LAWS: &str = "functions BOX_LAWS

specify {ANY}

  law bump_grows(b: BOX)
    require
      below: b.value < 100
    ensure
      grows: b.bump.value = b.value + 1
    end

end
";

/// A law of another module, used at the value of a call whose first argument is a `NAT`.
const USE: &str = "functions USE

feature {ANY}

  twice(n: NAT, b: BOX): BOX
    variant
      n
    do
      if n = 0 then b else USE.twice(n - 1, b).bump end
    end

specify {ANY}

  law bounded(b: BOX, n: NAT)
    require
      low: b.value + n < 100
    variant
      n
    do
      if n = 0 then True else
        bounded(b, n - 1) and
        BOX_LAWS.bump_grows(USE.twice(n - 1, b))
      end
    ensure
      grows: USE.twice(n, b).value = b.value + CLAIM
    end

end
";

/// The `LIST` facts at the result of a prelude routine with a `NAT` before a list.
const DIGITS: &str = "functions DIGITS

feature {ANY}

  joined(n: NAT, ys: LIST[U64], zs: LIST[U64]): NAT
    do
      NUMBER_TEXT.digits(n, ys).append(zs).length
    ensure
      sum: Result = NUMBER_TEXT.digits(n, ys).length + zs.length
    end

end
";

struct Project(PathBuf);

impl Project {
    fn new(name: &str, sources: &[(&str, String)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_call_value_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        let manifest = format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", name.replace('_', "-"));
        fs::write(dir.join("Gardel.toml"), manifest).expect("manifest");
        for (file, text) in sources {
            fs::write(dir.join("src").join(file), text).expect("source file");
        }
        Self(dir)
    }

    fn check(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&self.0)
            .env("GARDEL_CACHE_DIR", self.0.join("cache"))
            .args(["--project", ".", "--strict"])
            .output()
            .expect("gardelc runs")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn boxes(name: &str, claim: &str) -> Project {
    Project::new(
        name,
        &[
            ("box.gdl", BOX.to_string()),
            ("box_laws.gdl", BOX_LAWS.to_string()),
            ("use.gdl", USE.replace("CLAIM", claim)),
        ],
    )
}

#[test]
fn a_law_applies_at_the_value_of_a_call() {
    let out = boxes("law", "n").check();
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
}

/// The law still says only what it says.
#[test]
fn a_claim_past_the_law_is_refused() {
    let out = boxes("lie", "n + 1").check();
    assert!(!out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("condition `grows` failed"), "{}", all(&out));
}

#[test]
fn list_facts_apply_at_the_value_of_a_call() {
    let out = Project::new("digits", &[("digits.gdl", DIGITS.to_string())]).check();
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
}
