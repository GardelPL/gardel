//! Lists that carry their range (Task H.0b): an element of a `LIST` or `SEQ` of a machine
//! number type is in the type's range wherever it is read, a byte of a `BYTES` is below 256,
//! and a list built from a value out of range is refused where it is built.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, source: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_elements_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        fs::write(dir.join("Gardel.toml"), format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", name.replace('_', "-")))
            .expect("manifest");
        // One module per file, named after the module (§4.2).
        let module = source.split_whitespace().nth(1).expect("a module name").to_lowercase();
        fs::write(dir.join("src").join(format!("{module}.gdl")), source).expect("source file");
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

fn assert_ok(out: &Output) {
    assert!(out.status.success() && all(out).contains("0 errors, 0 warnings"), "{}", all(out));
}

/// Each routine must be refused, and by the clause named: `claim` for a lie, or the range
/// check of the list it builds.
fn assert_refused(out: &Output, failures: &[(&str, &str)]) {
    let text = all(out);
    assert!(!out.status.success(), "must fail:\n{text}");
    for (routine, tag) in failures {
        assert!(text.contains(&format!("in `{routine}` ({tag})")), "`{routine}` must fail `{tag}`:\n{text}");
    }
}

const READS: &str = "module READS

representation {ANY}
  items: LIST[U64]

create {ANY}

  first(xs: LIST[U64]): U64
    require
      ne: xs.length > 0
    do
      xs.head
    ensure
      lower: Result >= 0
    end

  second(xs: LIST[I64]): BOOL
    require
      ne: xs.length > 1
    do
      xs.tail.head >= -9223372036854775808 and xs.tail.head <= 9223372036854775807
    ensure
      holds: Result
    end

  at(xs: LIST[U64], i: NAT): BOOL
    require
      in: i < xs.length
    do
      LIST_OPS.at(xs, i) <= 18446744073709551615
    ensure
      holds: Result
    end

  member(cs: LIST[CHAR], c: CHAR): BOOL
    require
      has: cs.has(c)
    do
      c <= 1114111
    ensure
      holds: Result
    end

  folded(xs: LIST[U64]): BOOL
    do
      LIST_OPS.fold_left(xs, fn(acc: BOOL, x: U64): BOOL do acc and x >= 0 end, True)
    end

  binder(o: OPTION[U64]): BOOL
    do
      match o
        case Some(b) then b >= 0
        case None then True
      end
    ensure
      holds: Result
    end

  -- A generic routine gives back elements of its arguments, so their range.
  dropped(xs: LIST[U64], n: NAT): LIST[U64]
    require
      fits: n <= xs.length
    do
      LIST_OPS.drop(xs, n)
    end

  seq_head(s: SEQ[I64]): BOOL
    require
      ne: s.length > 0
    do
      s.to_list.head >= -9223372036854775808
    ensure
      holds: Result
    end

  seq_leaf(s: SEQ[U64]): BOOL
    do
      match s
        case Leaf(v) then v >= 0
        case Empty then True
        case Concat(l, r, n) then True
      end
    ensure
      holds: Result
    end

  built(x: U64, xs: LIST[U64]): READS
    require
      some: xs.length > 0
    do
      { items = xs.prepend(x).append(xs.tail) }
    end

  octet(b: BYTES): BOOL
    require
      ne: b.length > 0
    do
      b.model.head < 256 and b.model.tail.length < b.length
    ensure
      holds: Result
    end

  octet_at(b: BYTES, i: NAT): BOOL
    require
      in: i < b.length
    do
      LIST_OPS.at(b.model, i) < 256
    ensure
      holds: Result
    end

  -- The lines of a text are lists of characters, each in range.
  line_char(ls: LIST[LIST[CHAR]]): BOOL
    require
      ne: ls.length > 0
      line: ls.head.length > 0
    do
      ls.head.head.code >= 0 and LIST_OPS.at(ls, 0).tail.length < ls.head.length
    ensure
      holds: Result
    end

  -- A function gives a value in its result type's range for arguments in theirs.
  applied(f: (x: NAT) -> NAT, n: NAT): BOOL
    do
      f(n) >= 0
    ensure
      holds: Result
    end

  applied_lambda(n: NAT): BOOL
    do
      READS.applied(fn(x: NAT): NAT do x end, n)
    ensure
      holds: Result
    end

  doubled(xs: LIST[U64]): LIST[U64]
    do
      LIST_OPS.map(xs, fn(x: U64): U64 do x *% 2 end)
    end

feature {ANY}

  field_head: BOOL
    require
      ne: Current.items.length > 0
    do
      Current.items.head >= 0
    ensure
      holds: Result
    end

end
";

/// Every read of an element knows its range: `head`, `tail`, `at`, `has`, a fold's argument, a
/// match binder, the result of a generic routine, a `SEQ`'s list, a field, a list of lists and
/// a function's result. A byte read from `BYTES.model` is below 256.
#[test]
fn an_element_read_from_a_list_is_in_range() {
    assert_ok(&Project::new("reads", READS).check());
}

const BUILT: &str = "module BUILT

representation {ANY}
  items: LIST[U64]

create {ANY}

  big: LIST[U64]
    do
      LIST.empty.prepend(18446744073709551616)
    end

  from_int(n: MATH_INT): LIST[U64]
    do
      LIST.singleton(n)
    end

  apply(xs: LIST[MATH_INT]): LIST[U64]
    do
      xs
    end

  take(xs: LIST[U64]): BOOL
    do
      xs.length = 0
    end

  pass(xs: LIST[MATH_INT]): BOOL
    do
      BUILT.take(xs)
    end

  record(n: MATH_INT): BUILT
    do
      { items = LIST.singleton(n) }
    end

  nested(n: MATH_INT): LIST[LIST[U64]]
    do
      LIST.singleton(LIST.singleton(n))
    end

  use_fn(f: (x: NAT) -> NAT): BOOL
    do
      True
    end

  pass_fn(g: (x: NAT) -> MATH_INT): BOOL
    do
      BUILT.use_fn(g)
    end

  -- Were the record accepted, its field's range would prove this.
  lie(n: MATH_INT): BOOL
    require
      neg: n < 0
    do
      True
    ensure
      claim: BUILT.record(n).items.head >= 0
    end

end
";

const VARIANT: &str = "module CELL

representation {ANY}
  | Bytes(items: LIST[U64])
  | Nothing

create {ANY}

  of(n: MATH_INT): CELL
    do
      Bytes(LIST.singleton(n))
    end

end
";

/// A list of a machine type built from a value out of range is refused where it is built: as a
/// result, as an argument, in a field of a record or a variant, and inside a list of lists. A
/// function whose result has no range is refused where a function with one is expected. Before,
/// each was accepted, and the field's range then proved a false claim.
#[test]
fn a_list_built_out_of_range_is_refused_where_it_is_built() {
    assert_refused(
        &Project::new("built", BUILT).check(),
        &[
            ("big", "elements"),
            ("from_int", "elements"),
            ("apply", "elements"),
            ("pass", "xs_elements"),
            ("record", "items_elements"),
            ("nested", "elements"),
            ("pass_fn", "f_elements"),
        ],
    );
    assert_refused(&Project::new("variant", VARIANT).check(), &[("of", "items_elements")]);
}

const LIES: &str = "module LIES

create {ANY}

  -- A `LIST[U64]` is in the range of `U64`, not of a byte.
  not_a_byte(xs: LIST[U64]): BOOL
    require
      ne: xs.length > 0
    do
      True
    ensure
      claim: xs.head < 256
    end

  -- A byte is below 256, not below 128.
  not_ascii(b: BYTES): BOOL
    require
      ne: b.length > 0
    do
      True
    ensure
      claim: b.model.head < 128
    end

  -- A `LIST[MATH_INT]` has no range.
  unbounded(xs: LIST[MATH_INT]): BOOL
    require
      ne: xs.length > 0
    do
      True
    ensure
      claim: xs.head >= 0
    end

  -- The range of an `I64` list does not reach below its least value.
  signed(xs: LIST[I64]): BOOL
    require
      ne: xs.length > 0
    do
      True
    ensure
      claim: xs.head >= -9223372036854775807
    end

  -- A function of a `MATH_INT` result has no range.
  unbounded_fn(f: (x: NAT) -> MATH_INT, n: NAT): BOOL
    do
      True
    ensure
      claim: f(n) >= 0
    end

end
";

#[test]
fn a_range_that_does_not_hold_is_not_proven() {
    assert_refused(
        &Project::new("lies", LIES).check(),
        &[
            ("not_a_byte", "claim"),
            ("not_ascii", "claim"),
            ("unbounded", "claim"),
            ("signed", "claim"),
            ("unbounded_fn", "claim"),
        ],
    );
}
