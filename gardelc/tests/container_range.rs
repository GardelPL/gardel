//! A container of machine numbers holds them in their type's range (H.0e): an `OPTION[U64]`,
//! a `TUPLE[U64, BOOL]`, a `RESULT[STRING, U64]`, a `MAP[STRING, U64]` and a user generic
//! container are checked where they are built, and a number read out of one is in range with
//! no trust. Before H.0e, `Some(m)` with `m: MATH_INT` verified as an `OPTION[U64]`, while
//! `o.get` of one was trusted to be in range, so a caller could prove `False`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_containers_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        fs::write(dir.join("Gardel.toml"), format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", name.replace('_', "-")))
            .expect("manifest");
        for (file, source) in files {
            fs::write(dir.join("src").join(file), source).expect("source file");
        }
        Self { dir }
    }

    fn check(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&self.dir)
            .env("GARDEL_KEEP_BPL", "1")
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

/// A user generic container, with a field of its type parameter.
const CRATE: &str = "module CRATE[T]

representation {ANY}
  item: T
  count: NAT

create {ANY}

  make(x: T): CRATE[T]
    do
      { item = x, count = 1 }
    end

feature {ANY}

  get: T
    do
      Current.item
    end

  -- Its body unfolds only as far as the fuel goes, so a caller needs the range it keeps.
  pick_from(c: CRATE[T], n: NAT): T
    variant
      n
    do
      if n = 0 then c.item else Current.pick_from(c, n - 1) end
    end

end
";

/// A record with a field of a container type.
const HOLDER: &str = "module HOLDER

representation {ANY}
  held: OPTION[U64]

create {ANY}

  make(m: MATH_INT): HOLDER
    do
      { held = Some(m) }
    end

end
";

const SMALL_HOLDER: &str = "module HOLDER

representation {ANY}
  held: OPTION[U64]

create {ANY}

  make(m: MATH_INT): HOLDER
    require
      small: m >= 0 and m < 100
    do
      { held = Some(m) }
    end

end
";

const LIES: &str = "module WORK

create {ANY}

  some_out(m: MATH_INT): OPTION[U64]
    do
      Some(m)
    end

  pair_out(m: MATH_INT): TUPLE[U64, BOOL]
    do
      (m, True)
    end

  ok_out(m: MATH_INT): RESULT[STRING, U64]
    do
      Ok(m)
    end

  map_out(m: MATH_INT): MAP[STRING, U64]
    do
      MAP.empty.put(\"a\", m)
    end

  crate_out(m: MATH_INT): CRATE[U64]
    do
      CRATE.make(m)
    end

  nested_out(m: MATH_INT): OPTION[LIST[U64]]
    do
      Some(LIST.empty.prepend(m))
    end

  takes(o: OPTION[U64]): U64
    do
      o.unwrap_or(0)
    end

  pass_out(m: MATH_INT): U64
    do
      takes(Some(m))
    end

  fold_out(m: MAP[STRING, NAT], x: MATH_INT): NAT
    do
      m.put(\"a\", x).fold(0, fn(acc: NAT, k: STRING, v: NAT): NAT do v end)
    end

  let_out(m: MAP[STRING, NAT], x: MATH_INT): NAT
    do
      let y = m.put(\"a\", x)
      y.get(\"a\").unwrap_or(0)
    end

end
";

const TRUTHS: &str = "module WORK

create {ANY}

  pick(o: OPTION[NAT], d: NAT): NAT
    do
      o.unwrap_or(d)
    end

  got(o: OPTION[U64]): U64
    require
      some: o.is_some
    do
      o.get
    end

  first_of(p: TUPLE[U64, BOOL]): U64
    do
      p.first
    end

  inside(o: OPTION[U64]): U64
    do
      match o
        case Some(x) then x
        case None then 0
      end
    end

  ok_of(r: RESULT[STRING, U64]): U64
    do
      r.unwrap_or(0)
    end

  lookup(m: MAP[STRING, NAT], k: STRING): NAT
    do
      m.get(k).unwrap_or(0)
    end

  crate_of(c: CRATE[U64]): U64
    do
      c.get
    end

  nested(o: OPTION[LIST[U64]]): U64
    do
      match o
        case Some(xs) then if xs.length > 0 then xs.head else 0 end
        case None then 0
      end
    end

  some_in(x: U64): OPTION[U64]
    do
      Some(x)
    end

  pair_in(x: U64): TUPLE[U64, BOOL]
    do
      (x, True)
    end

  ok_in(x: U64): RESULT[STRING, U64]
    do
      Ok(x)
    end

  map_in(x: U64): MAP[STRING, U64]
    do
      MAP.empty.put(\"a\", x)
    end

  crate_in(x: U64): CRATE[U64]
    do
      CRATE.make(x)
    end

  nested_in(x: U64): OPTION[LIST[U64]]
    do
      Some(LIST.empty.prepend(x))
    end

  takes(o: OPTION[U64]): U64
    do
      o.unwrap_or(0)
    end

  pass_in(x: U64): U64
    do
      takes(Some(x))
    end

  options_in(x: U64): MAP[STRING, OPTION[U64]]
    do
      MAP.empty.put(\"a\", Some(x))
    end

  both(r: RESULT[U64, TUPLE[U64, U64]]): U64
    do
      match r
        case Ok((a, _)) then a
        case Error(e) then e
      end
    end

  deep(m: MAP[STRING, OPTION[U64]], k: STRING): U64
    do
      m.get(k).unwrap_or(None).unwrap_or(0)
    end

  list_in(xs: LIST[U64]): OPTION[LIST[U64]]
    do
      Some(xs)
    end

  list_of(o: OPTION[LIST[U64]]): LIST[U64]
    do
      o.unwrap_or(LIST.empty)
    end

  crate_field(c: CRATE[U64]): U64
    do
      c.item
    end

  crate_other(a: CRATE[U64], b: CRATE[U64], n: NAT): U64
    do
      a.pick_from(b, n)
    end

  crate_deep(c: CRATE[OPTION[U64]], n: NAT): U64
    do
      match c.pick_from(c, n)
        case Some(x) then x
        case None then 0
      end
    end

end
";

/// Each container built from a number out of range is refused where it is built.
#[test]
fn a_container_of_a_number_out_of_range_is_refused_where_it_is_built() {
    let p = Project::new("lies", &[("work.gdl", LIES), ("crate.gdl", CRATE), ("holder.gdl", HOLDER)]);
    let out = p.check();
    let text = all(&out);
    assert!(!out.status.success(), "{text}");
    for (routine, tag) in [
        ("some_out", "elements"),
        ("pair_out", "elements"),
        ("ok_out", "elements"),
        ("map_out", "elements"),
        ("crate_out", "elements"),
        ("nested_out", "elements"),
        ("pass_out", "o_elements"),
        // A value given to a generic parameter, as `put`'s `v: V`, is not checked, but nothing
        // read back out of the map is in range then.
        ("fold_out", "result"),
        ("let_out", "result"),
        ("make", "held_elements"),
    ] {
        assert!(
            text.contains(&format!("Range Violation) in `{routine}` ({tag})")),
            "`{routine}` is not refused for `{tag}`:\n{text}"
        );
    }
    assert!(!text.contains("in `takes`"), "{text}");
}

/// A container built from numbers in range is accepted, and a number read out of one is in
/// range with no trust.
#[test]
fn a_number_read_out_of_a_container_is_in_range() {
    let p = Project::new("truths", &[("work.gdl", TRUTHS), ("crate.gdl", CRATE), ("holder.gdl", SMALL_HOLDER)]);
    let out = p.check();
    let text = all(&out);
    assert!(out.status.success() && text.contains("0 errors, 0 warnings"), "{text}");

    // No read is assumed in range: the only assumption of a range is a list's length.
    let bpl = fs::read_to_string(p.dir.join("build").join("work.bpl")).expect("the kept .bpl");
    for line in bpl.lines().filter(|l| l.trim_start().starts_with("assume in_")) {
        assert!(line.contains("Seq_length("), "a read is trusted: {line}\n{bpl}");
    }
    assert!(bpl.contains("Rng_holds(Rng__OPTION_A(Rng_u64), o)"), "{bpl}");
}
