//! A machine number is in its type's range wherever a routine, a caller or a reader of a
//! field assumes it (H.0d): a routine's result, an argument and a field are checked where
//! they are made. Before H.0d, `big: U64 do 18446744073709551616 end` verified, and its frame
//! axiom `in_u64(fn_big)` contradicted its definition.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_range_{}_{name}", std::process::id()));
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

const BOX: &str = "module BOX

representation {ANY}
  | Box(value: U64)
  | Empty

end
";

const CELL: &str = "module CELL

representation {ANY}
  value: U64

create {ANY}

  make(m: MATH_INT): CELL
    do
      { value = m }
    end

  make_small(m: MATH_INT): CELL
    require
      small: m >= 0 and m < 100
    do
      { value = m }
    end

end
";

const SMALL_CELL: &str = "module CELL

representation {ANY}
  value: U64

create {ANY}

  make_small(m: MATH_INT): CELL
    require
      small: m >= 0 and m < 100
    do
      { value = m }
    end

end
";

const LIES: &str = "module WORK

create {ANY}

  big: U64
    do
      18446744073709551616
    end

  negative: U64
    do
      -1
    end

  take(x: U64): U64
    do
      x
    end

  pass(m: MATH_INT): U64
    do
      take(m)
    end

  wrap(m: MATH_INT): U64
    do
      m
    end

  through_local: U64
    do
      let x = 18446744073709551616
      take(x)
    end

  boxed(m: MATH_INT): BOX
    do
      Box(m)
    end

  total(s: SEQ[U64]): U64
    do
      s.fold_left(fn(acc: MATH_INT, x: U64): MATH_INT do acc + x end, 0)
    end

  start_out(s: SEQ[U64]): U64
    do
      s.fold_left(fn(acc: U64, x: U64): U64 do acc end, 18446744073709551616)
    end

  map_start_out(m: MAP[STRING, NAT]): NAT
    do
      m.fold(18446744073709551616, fn(acc: NAT, k: STRING, v: NAT): NAT do acc end)
    end

  pass_fold(s: SEQ[U64]): U64
    do
      take(s.fold_left(fn(acc: U64, x: U64): U64 do acc end, 18446744073709551616))
    end

  needs_small(x: U64): U64
    require
      small: x < 10
    do
      x
    end

  asks_big: U64
    do
      needs_small(20)
    end

end
";

const TRUTHS: &str = "module WORK

create {ANY}

  largest: U64
    do
      18446744073709551615
    end

  take(x: U64): U64
    do
      x
    end

  pass(m: MATH_INT): U64
    require
      small: m >= 0 and m < 100
    do
      take(m)
    end

  wrap(m: MATH_INT): U64
    require
      small: m >= 0 and m < 100
    do
      m
    end

  through_local(x: U64): U64
    do
      let y = x
      take(y)
    end

  boxed(x: U64): BOX
    do
      Box(take(x))
    end

  widen(x: NAT): U64
    do
      take(x)
    end

  count(xs: LIST[U64]): NAT
    do
      xs.length
    end

  pick(o: OPTION[NAT], d: NAT): NAT
    do
      o.unwrap_or(d)
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

  count_big(xs: LIST[U64]): NAT
    do
      LIST_OPS.filter(xs, fn(x: U64): BOOL do x > 3 end).length
    end

  count_doubled(xs: LIST[BOX]): NAT
    do
      LIST_OPS.map(xs, fn(b: BOX): U64 do 2 end).length
    end

  digits(s: SEQ[U64]): U64
    do
      s.fold_left(fn(acc: U64, x: U64): U64 do acc *% 10 +% x end, 0)
    end

  digits_right(s: SEQ[U64]): U64
    do
      s.fold_right(fn(x: U64, acc: U64): U64 do acc *% 10 +% x end, 0)
    end

  count_true(s: SEQ[BOOL]): U64
    do
      s.fold_left(fn(acc: U64, b: BOOL): U64 do if b then acc +% 1 else acc end end, 0)
    end

  wrapped(x: U64): U64
    do
      1 +% x *% 10
    end

  map_total(m: MAP[STRING, NAT]): NAT
    do
      m.fold(0, fn(acc: NAT, k: STRING, v: NAT): NAT do if acc <= 1000 and v <= 1000 then acc + v else acc end end)
    end

end
";

/// Each value out of its type's range is refused where it is made, and nowhere else.
#[test]
fn a_machine_number_out_of_range_is_refused_where_it_is_made() {
    let p = Project::new("lies", &[("work.gdl", LIES), ("box.gdl", BOX), ("cell.gdl", CELL)]);
    let out = p.check();
    let text = all(&out);
    assert!(!out.status.success(), "{text}");
    for (routine, tag) in [
        ("big", "result"),
        ("negative", "result"),
        ("pass", "x_range"),
        ("wrap", "result"),
        ("through_local", "x_range"),
        ("boxed", "value"),
        ("make", "value"),
        ("total", "result"),
        ("start_out", "result"),
        ("map_start_out", "result"),
        ("pass_fold", "x_range"),
    ] {
        assert!(
            text.contains(&format!("Range Violation) in `{routine}` ({tag})")),
            "`{routine}` is not refused for `{tag}`:\n{text}"
        );
    }
    assert!(!text.contains("in `take`") && !text.contains("in `make_small`"), "{text}");
    // A call whose precondition fails is refused there, and its result is not checked again.
    assert!(text.contains("in `asks_big` (small)"), "{text}");
    assert!(!text.contains("in `asks_big` (result)"), "{text}");
}

/// A value whose range follows from a `require`, a parameter or a call is accepted.
#[test]
fn a_machine_number_in_range_is_accepted() {
    let p = Project::new("truths", &[("work.gdl", TRUTHS), ("box.gdl", BOX), ("cell.gdl", SMALL_CELL)]);
    let out = p.check();
    let text = all(&out);
    assert!(out.status.success() && text.contains("0 errors, 0 warnings"), "{text}");

    // An argument whose own type has the range is not checked again: a parameter, and a
    // call's result. A `MATH_INT` is.
    let bpl = fs::read_to_string(p.dir.join("build").join("work.bpl")).expect("the kept .bpl");
    assert!(bpl.contains("pass::range::x_range"), "{bpl}");
    // A wrapping result is in range by construction: checking it costs Z3 minutes.
    for unchecked in ["widen::range::x_range", "boxed::range::x_range", "boxed::range::value", "wrapped::range::result"] {
        assert!(!bpl.contains(unchecked), "`{unchecked}` is checked:\n{bpl}");
    }
    // A value read out of a generic container is trusted to be in range. A routine that
    // declares a machine result is not: its body is checked.
    assert!(bpl.contains("assume in_u64("), "{bpl}");
    assert!(!bpl.contains("assume in_u64(fn_take("), "a declared result is trusted:\n{bpl}");
    // A lambda translated twice, by the obligations and by the body, keeps one name.
    assert_eq!(bpl.matches("function lam_WORK_").count(), TRUTHS.matches("fn(").count(), "{bpl}");
}
