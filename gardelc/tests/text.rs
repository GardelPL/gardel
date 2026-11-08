//! Text, bytes and numbers (Task F.2): what the C primitives compute matches their verified
//! models, the round trips hold, and no false contract gets through the new rules.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, sources: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_text_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        let package = name.replace('_', "-");
        let manifest = format!("[project]\nname = \"{package}\"\nversion = \"0.1.0\"\n");
        fs::write(dir.join("Gardel.toml"), manifest).expect("manifest");
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

    fn run_binary(&self) -> String {
        let exe = self.dir.join("build").join("main_bin.exe");
        let out = Command::new(exe).output().expect("the program runs");
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).expect("UTF-8 output").replace("\r\n", "\n")
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
    assert!(out.status.success(), "{}", all(out));
    assert!(all(out).contains("0 errors, 0 warnings"), "{}", all(out));
}

fn assert_refused(out: &Output, text: &str) {
    assert!(!out.status.success(), "must fail with {text}:\n{}", all(out));
    assert!(all(out).contains(text), "must say {text}:\n{}", all(out));
}

fn client(routines: &str) -> String {
    format!("module CLIENT\n\nrepresentation {{ANY}}\n  n: NAT\n\ncreate {{ANY}}\n\n{routines}\nend\n")
}

/// Every operation, run in C. The primitives (`slice`, `concat`, `less_than`, `to_list`,
/// `from_list`, the builder's join) are hand-written, so this compares them with what their
/// verified models say, on multi-byte text and at every number's edge.
#[test]
fn the_runtime_computes_what_the_models_say() {
    let p = Project::new("runtime", &[("main.gdl", include_str!("text_program.gdl"))]);
    assert_ok(&p.check());
    let expected = "\
ïve c | café | 6 | 9 | none
yes | yes | no
padded | [ |  | ]
[a][b][][c] | [] | yes | yes | no | no
2 | 2 | none | yes | cab | abcabc-x | abcé! | xyz
0 | 18446744073709551615 | -9223372036854775808 | 9223372036854775807 | -42 | 18446744073709551615 | out of range | 7 | not a digit at | 2 | no digits | -9223372036854775808 | out of range | out of range | no digits | not a digit at | 1 | beef | 48879 | none
";
    assert_eq!(p.run_binary(), expected);
}

#[test]
fn a_lie_about_text_does_not_verify() {
    let lies = [
        ("round_trip", "f(n: NAT): BOOL\n    require\n      small: n < 100\n    do\n      True\n    ensure\n      claim: NUMBER_TEXT.parse_nat(NUMBER_TEXT.nat_to_text(n)) = Ok(n + 1)\n    end\n"),
        ("i64_round_trip", "f(i: I64): BOOL\n    require\n      negative: i < 0\n    do\n      True\n    ensure\n      claim: NUMBER_TEXT.parse_i64(NUMBER_TEXT.i64_to_text(i)) = Ok(0 - i)\n    end\n"),
        ("order", "f(a: STRING, b: STRING): BOOL\n    do\n      True\n    ensure\n      claim: not (a.less_than(b) and b.less_than(a)) implies a.less_than(b)\n    end\n"),
        ("both_ways", "f(a: STRING, b: STRING): BOOL\n    require\n      ab: a.less_than(b)\n    do\n      True\n    ensure\n      claim: b.less_than(a)\n    end\n"),
        ("slice", "f(s: STRING): BOOL\n    require\n      long: s.code_point_count >= 2\n    do\n      True\n    ensure\n      claim: s.slice(0, 1).code_point_count = 2\n    end\n"),
        ("find", "f(s: STRING, t: STRING): BOOL\n    do\n      True\n    ensure\n      claim: s.find(t, 0).is_some\n    end\n"),
        ("builder", "f(x: NAT): BOOL\n    do\n      True\n    ensure\n      claim: BYTES_BUILDER.empty.add_text(\"ab\").to_bytes.length = 3\n    end\n"),
        // A machine type is a range of Boogie's `int`: a witness outside it must not count. The
        // solver proves an `exists` only from a term that matches its body, so the witness is a
        // call in the `require`. `exists k: NAT. k < 0` or `k = i` fails even with no range.
        ("exists_nat", "neg(k: MATH_INT): BOOL\n    do\n      k < 0\n    end\n\n  f(i: MATH_INT): BOOL\n    require\n      witness: CLIENT.neg(i)\n    do\n      True\n    ensure\n      claim: exists k: NAT. CLIENT.neg(k)\n    end\n"),
        ("exists_i64", "big(k: MATH_INT): BOOL\n    do\n      k > 9223372036854775807\n    end\n\n  f(i: MATH_INT): BOOL\n    require\n      witness: CLIENT.big(i)\n    do\n      True\n    ensure\n      claim: exists k: I64. CLIENT.big(k)\n    end\n"),
    ];
    for (name, routine) in lies {
        let p = Project::new(&format!("lie_{name}"), &[("client.gdl", &client(&format!("  {routine}")))]);
        assert_refused(&p.check(), "claim");
    }
}

#[test]
fn what_the_contracts_give_a_client() {
    let facts = client(
        "  pieces(s: STRING): NAT
    require
      long: s.code_point_count >= 3
    do
      s.slice(1, 3).code_point_count
    ensure
      two: Result = 2
    end

  ordered(a: STRING, b: STRING, c: STRING): BOOL
    require
      ab: a.less_than(b)
      bc: b.less_than(c)
    do
      True
    ensure
      ac: a.less_than(c)
      not_ba: not b.less_than(a)
    end

  built(x: NAT): NAT
    do
      BYTES_BUILDER.empty.add_text(\"ab\").add_byte(33).to_bytes.length
    ensure
      three: Result = 3
    end

  read_back(n: NAT): RESULT[NUMBER_ERROR, NAT]
    do
      NUMBER_TEXT.parse_nat(NUMBER_TEXT.nat_to_text(n))
    ensure
      same: Result = Ok(n)
    end

  first_length(xs: LIST[BYTES]): NAT
    require
      some: xs.length > 0
    do
      xs.head.length
    ensure
      model: Result = xs.head.to_list.length
    end

  in_range(z: NAT): BOOL
    do
      True
    ensure
      natural: forall k: NAT. k >= 0
    end
",
    );
    assert_ok(&Project::new("facts", &[("client.gdl", &facts)]).check());
}

/// `STRING` satisfies `ORD`, so a generic bounded by it accepts text, and links its order.
#[test]
fn text_is_a_key() {
    let sorted = "module SORTED[K: ORD]

representation {ANY}
  item: K

create {ANY}

  smaller(a: K, b: K): K
    do
      if a.less_than(b) then a else b end
    ensure
      least: not b.less_than(Result) and not a.less_than(Result)
    end

end
";
    let use_it = client(
        "  first(a: STRING, b: STRING): STRING\n    require\n      ab: a.less_than(b)\n    do\n      SORTED.smaller(a, b)\n    ensure\n      is_a: Result.equals(a)\n    end\n",
    );
    let p = Project::new("key", &[("sorted.gdl", sorted), ("client.gdl", &use_it)]);
    assert_ok(&p.check());
}

#[test]
fn a_number_conversion_needs_its_range() {
    let unguarded = [
        ("to_nat", "f(i: I64): NAT\n    do\n      NAT.from_i64(i)\n    end\n"),
        ("to_i64", "f(n: NAT): I64\n    do\n      I64.from_nat(n)\n    end\n"),
        // One past each edge: the range is exact, not merely present.
        ("to_nat_edge", "f(i: I64): NAT\n    require\n      edge: i >= 0 - 1\n    do\n      NAT.from_i64(i)\n    end\n"),
        ("to_i64_edge", "f(n: NAT): I64\n    require\n      edge: n <= 9223372036854775808\n    do\n      I64.from_nat(n)\n    end\n"),
    ];
    for (name, routine) in unguarded {
        let p = Project::new(&format!("conv_{name}"), &[("client.gdl", &client(&format!("  {routine}")))]);
        assert_refused(&p.check(), "in_range");
    }
    let guarded = client(
        "  f(i: I64): NAT\n    require\n      natural: i >= 0\n    do\n      NAT.from_i64(i)\n    ensure\n      same: Result = i\n    end\n\n  g(n: NAT): I64\n    require\n      small: n <= 9223372036854775807\n    do\n      I64.from_nat(n)\n    ensure\n      same: Result = n\n    end\n",
    );
    assert_ok(&Project::new("conv_ok", &[("client.gdl", &guarded)]).check());
}
