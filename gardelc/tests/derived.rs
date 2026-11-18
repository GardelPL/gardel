//! Derived instances for enumerations (Task H.0c): a sum whose variants have no fields gets
//! `equals`, `less_than`, `ordinal`, `from_ordinal`, `all` and the concept's laws from the
//! compiler, each verified as if written by hand. A sum with fields must write its own.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, sources: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_derived_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        fs::write(dir.join("Gardel.toml"), format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", name.replace('_', "-")))
            .expect("manifest");
        for (file, text) in sources {
            fs::write(dir.join("src").join(file), text).expect("source file");
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

    fn generated(&self, file: &str) -> String {
        fs::read_to_string(self.dir.join("build").join(file)).expect("build output")
    }

    fn run_binary(&self) -> i32 {
        let exe = self.dir.join("build").join("main_bin.exe");
        Command::new(exe).status().expect("the program runs").code().expect("an exit code")
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

fn assert_ok(out: &Output) {
    assert!(out.status.success() && all(out).contains("0 errors, 0 warnings"), "{}", all(out));
}

/// Each routine must be refused, by the clause or rule named.
fn assert_refused(out: &Output, failures: &[(&str, &str)]) {
    let text = all(out);
    assert!(!out.status.success(), "must fail:\n{text}");
    for (routine, tag) in failures {
        assert!(text.contains(&format!("in `{routine}` ({tag})")), "`{routine}` must fail `{tag}`:\n{text}");
    }
}

const LOG_LEVEL: &str = "module LOG_LEVEL satisfies ORD

representation {ANY}
  | Debug
  | Info
  | Warn
  | Error

end
";

const CLIENT: &str = "module CLIENT

create {ANY}

  ordered: BOOL
    do
      Debug.less_than(Error) and Info.less_or_equal(Info) and not Warn.less_than(Info)
    ensure
      holds: Result
    end

  numbered: BOOL
    do
      Warn.ordinal = 2 and LOG_LEVEL.from_ordinal(3) = Some(Error) and LOG_LEVEL.from_ordinal(4) = None
    ensure
      holds: Result
    end

  listed(t: LOG_LEVEL): BOOL
    do
      LOG_LEVEL.all_listed(t) and LOG_LEVEL.all_at_ordinal(t) and
        LOG_LEVEL.all.length = 4 and LOG_LEVEL.all.has(t) and LIST_OPS.at(LOG_LEVEL.all, t.ordinal) = t
    ensure
      holds: Result
    end

  equal(a: LOG_LEVEL, b: LOG_LEVEL): BOOL
    require
      same: a.equals(b)
    do
      a.ordinal = b.ordinal
    ensure
      holds: Result
    end

  -- The derived order is an `ORD`, so a `MAP` takes it as its key.
  counted(m: MAP[LOG_LEVEL, NAT]): OPTION[NAT]
    do
      m.put(Warn, 7).get(Warn)
    ensure
      found: Result = Some(7)
    end

end
";

const MAIN: &str = "program MAIN

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let m = MAP.empty.put(Error, 3).put(Debug, 1)
      let first = LIST_OPS.at(m.keys, 0).ordinal
      let n = LOG_LEVEL.all.length
      let o = match LOG_LEVEL.from_ordinal(2)
        case Some(l) then l.ordinal
        case None then 9
      end
      let lt = if Info.less_than(Warn) and not Error.less_than(Debug) and Warn.equals(Warn) then 1 else 0 end
      with IO
        IO.exit(first * 1000 +% n * 100 +% o * 10 +% lt)
      end
    end

end
";

/// `module LOG_LEVEL satisfies ORD` with four variants and no feature verifies, and a client
/// proves the order, the ordinals, `all` and the derived `equals`, and uses `LOG_LEVEL` as the
/// key of a `MAP`. The C program computes the same.
#[test]
fn an_enumeration_gets_its_instance() {
    let p = Project::new("instance", &[("log_level.gdl", LOG_LEVEL), ("client.gdl", CLIENT), ("main.gdl", MAIN)]);
    assert_ok(&p.check());
    // Every law is an obligation of `LOG_LEVEL`'s own, `CANONICAL_EQ`'s too, which the
    // derived `equals` brings though the module names only `ORD`.
    let bpl = p.generated("log_level.bpl");
    for law in [
        "ordinal_back",
        "all_listed",
        "all_at_ordinal",
        "CANONICAL_EQ_agrees_with_structural",
        "EQ_reflexive",
        "EQ_symmetric",
        "EQ_transitive",
        "ORD_irreflexive",
        "ORD_transitive",
        "ORD_total",
    ] {
        assert!(bpl.contains(&format!("procedure Verify_law_{law}(")), "`{law}` must be proven:
{bpl}");
    }
    // The map's first key is `Debug` (0); four levels; `Warn` is 2; `Info < Warn`.
    assert_eq!(p.run_binary(), 421);
}

const LIES: &str = "module LIES

create {ANY}

  backwards: BOOL
    do
      True
    ensure
      claim: Error.less_than(Debug)
    end

  five: BOOL
    do
      True
    ensure
      claim: LOG_LEVEL.all.length = 5
    end

  past_the_end: BOOL
    do
      True
    ensure
      claim: LOG_LEVEL.from_ordinal(4) = Some(Error)
    end

  same(a: LOG_LEVEL, b: LOG_LEVEL): BOOL
    do
      True
    ensure
      claim: a.equals(b)
    end

end
";

/// What the derived instance does not say is not proven.
#[test]
fn a_derived_instance_proves_no_more_than_it_says() {
    let p = Project::new("lies", &[("log_level.gdl", LOG_LEVEL), ("lies.gdl", LIES)]);
    assert_refused(
        &p.check(),
        &[("backwards", "claim"), ("five", "claim"), ("past_the_end", "claim"), ("same", "claim")],
    );
}

/// A routine written by hand wins over the derived one, and the laws are proven for it, so a
/// wrong one is refused: an `ordinal` that puts two levels at 0 breaks the order's totality and
/// the way back from an ordinal, and an `equals` that is always true breaks `CANONICAL_EQ` when `less_than` is derived beside it.
#[test]
fn a_derived_law_is_proven_not_trusted() {
    let flat = "module FLAT satisfies ORD

representation {ANY}
  | Low
  | High

feature {ANY}

  ordinal: NAT
    do
      0
    end

end
";
    let loose = "module LOOSE satisfies ORD, CANONICAL_EQ

representation {ANY}
  | One
  | Two

feature {ANY}

  equals(other: LOOSE): BOOL
    do
      True
    end

end
";
    assert_refused(
        &Project::new("flat", &[("flat.gdl", flat)]).check(),
        &[("ORD_total", "comparable"), ("ordinal_back", "back"), ("all_at_ordinal", "placed")],
    );
    assert_refused(&Project::new("loose", &[("loose.gdl", loose)]).check(), &[("CANONICAL_EQ_agrees_with_structural", "sound")]);
}

/// Only an enumeration gets an instance from the compiler: a sum with a field and a record
/// must write theirs (`E0323`). A method a concept asks for that the compiler does not derive
/// is still missing.
#[test]
fn a_type_with_fields_must_write_its_instance() {
    let cell = "module CELL satisfies CANONICAL_EQ

representation {ANY}
  | Full(n: NAT)
  | Empty

end
";
    let point = "module POINT satisfies ORD

representation {ANY}
  x: NAT

feature {ANY}

  equals(other: POINT): BOOL
    do
      Current.x = other.x
    end

end
";
    for (name, src, method) in [("cell", cell, "equals"), ("point", point, "less_than")] {
        let out = Project::new(name, &[(&format!("{name}.gdl"), src)]).check();
        let text = all(&out);
        assert!(!out.status.success(), "{text}");
        assert!(text.contains(&format!("E0323: underivable_instance")) && text.contains(&format!("writes no `{method}`")), "{text}");
    }
}
