//! Concept bounds through the command line (Task F.1): a method from a bound means the instance
//! chosen at each use, a bound is checked at each use, and no false contract gets through one.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const SORTED: &str = "module SORTED[K: ORD]

representation {ANY}
  item: K

create {ANY}

  make(k: K): SORTED[K]
    do
      { item = k }
    end

  smaller(a: K, b: K): K
    do
      if a.less_than(b) then a else b end
    ensure
      either: Result = a or Result = b
      least: not b.less_than(Result) and not a.less_than(Result)
    end

feature {ANY}

  at_most(b: K): BOOL
    do
      Current.item.less_or_equal(b)
    ensure
      said: Result = Current.item.less_or_equal(b)
    end

end
";

/// An order that is not the order of its numbers, so that a proof or a run that used `<` on
/// the rank by mistake would be caught.
const LEVEL: &str = "module LEVEL satisfies ORD

representation {ANY}
  rank: NAT

create {ANY}

  of(n: NAT): LEVEL
    do
      { rank = n }
    end

feature {ANY}

  equals(other: LEVEL): BOOL
    do
      Current.rank = other.rank
    end

  less_than(other: LEVEL): BOOL
    do
      Current.rank > other.rank
    end

specify {EQ}

  law reflexive(a: LEVEL)
    ensure
      refl: a.equals(a)
    end

  law symmetric(a: LEVEL, b: LEVEL)
    require
      ab: a.equals(b)
    ensure
      ba: b.equals(a)
    end

  law transitive(a: LEVEL, b: LEVEL, c: LEVEL)
    require
      ab: a.equals(b)
      bc: b.equals(c)
    ensure
      ac: a.equals(c)
    end

specify {ORD}

  law irreflexive(a: LEVEL)
    ensure
      not_less_than_self: not a.less_than(a)
    end

  law transitive(a: LEVEL, b: LEVEL, c: LEVEL)
    require
      a_lt_b: a.less_than(b)
      b_lt_c: b.less_than(c)
    ensure
      a_lt_c: a.less_than(c)
    end

  law total(a: LEVEL, b: LEVEL)
    ensure
      comparable: a.less_than(b) or a.equals(b) or b.less_than(a)
    end

end
";

const ROUTE: &str = "module ROUTE

representation {ANY}
  id: NAT
  handler: NAT -> NAT

create {ANY}

  make(i: NAT, h: NAT -> NAT): ROUTE
    do
      { id = i, handler = h }
    end

feature {ANY}

  run(x: NAT): NAT
    do
      Current.handler(x)
    end

end
";

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, sources: &[(&str, &str)]) -> Self {
        Self::with_manifest(name, "", sources)
    }

    fn with_manifest(name: &str, deps: &str, sources: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_bounds_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        let package = name.replace('_', "-");
        let manifest = format!("[project]\nname = \"{package}\"\nversion = \"0.1.0\"\n{deps}");
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

    fn generated(&self, file: &str) -> String {
        fs::read_to_string(self.dir.join("build").join(file)).expect("build output")
    }

    fn run_binary(&self) -> i32 {
        let exe = self.dir.join("build").join("main_bin.exe");
        let status = Command::new(exe).status().expect("the program runs");
        status.code().expect("an exit code")
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
    format!("module CLIENT\n\nfeature {{ANY}}\n\n{routines}\nend\n")
}

/// A generic module with one routine, and the bound in `head`.
fn generic(head: &str, routine: &str) -> String {
    format!("module {head}\n\nrepresentation {{ANY}}\n  item: K\n\ncreate {{ANY}}\n\n{routine}\nend\n")
}

#[test]
fn a_bound_method_means_the_instance_chosen_at_each_call() {
    let main = "program MAIN

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let low = SORTED.smaller(7, 3)
      let high = SORTED.smaller(LEVEL.of(7), LEVEL.of(3))
      with IO
        IO.exit(low * 10 +% high.rank)
      end
    end

end
";
    let use_both = client(
        "  nat_min(a: NAT, b: NAT): NAT
    do
      SORTED.smaller(a, b)
    ensure
      low: Result <= a and Result <= b
    end

  level_min(a: LEVEL, b: LEVEL): LEVEL
    do
      SORTED.smaller(a, b)
    ensure
      top: Result.rank >= a.rank and Result.rank >= b.rank
    end

  level_within(s: SORTED[LEVEL], b: LEVEL): BOOL
    do
      s.at_most(b)
    ensure
      said: Result = s.item.less_or_equal(b)
      unfolded: Result = (s.item.rank >= b.rank)
    end

  nat_within(s: SORTED[NAT], b: NAT): BOOL
    do
      s.at_most(b)
    ensure
      said: Result = (s.item <= b)
    end
",
    );
    let p = Project::new(
        "instances",
        &[
            ("sorted.gdl", SORTED),
            ("level.gdl", LEVEL),
            ("client.gdl", &use_both),
            ("main.gdl", main),
        ],
    );
    assert_ok(&p.check());
    // `SORTED` was proven once. `3` is the smaller NAT, and `7` the smaller LEVEL.
    assert!(p.generated("client.bpl").contains("SORTED__K_less(_bx, _by) <==> LEVEL__fn_less_than"));
    assert_eq!(p.run_binary(), 37);
}

#[test]
fn a_lie_through_a_bound_does_not_verify() {
    let lying_generic = [
        ("order", "smaller_of(a: K, b: K): BOOL\n    do\n      True\n    ensure\n      claim: a.less_than(b)\n    end\n"),
        ("self", "self_below(a: K): BOOL\n    do\n      True\n    ensure\n      claim: a.less_than(a)\n    end\n"),
        ("not_eq", "both(a: K, b: K): BOOL\n    do\n      True\n    ensure\n      claim: not a.equals(b)\n    end\n"),
        ("eq_is_not_identity", "same(a: K, b: K): BOOL\n    require\n      eq: a.equals(b)\n    do\n      True\n    ensure\n      claim: a = b\n    end\n"),
    ];
    for (name, routine) in lying_generic {
        let text = generic("LIE[K: ORD]", &format!("  {routine}"));
        let p = Project::new(&format!("generic_{name}"), &[("lie.gdl", &text)]);
        assert_refused(&p.check(), "claim");
    }

    let lying_client = [
        ("nat", "low_is_high(a: NAT, b: NAT): NAT\n    do\n      SORTED.smaller(a, b)\n    ensure\n      claim: Result >= a and Result >= b\n    end\n"),
        ("level", "uses_the_rank(a: LEVEL, b: LEVEL): LEVEL\n    do\n      SORTED.smaller(a, b)\n    ensure\n      claim: Result.rank <= a.rank\n    end\n"),
        ("literal", "seven(z: NAT): NAT\n    do\n      SORTED.smaller(3, 7)\n    ensure\n      claim: Result = 7\n    end\n"),
    ];
    for (name, routine) in lying_client {
        let text = client(&format!("  {routine}"));
        let p = Project::new(
            &format!("client_{name}"),
            &[("sorted.gdl", SORTED), ("level.gdl", LEVEL), ("client.gdl", &text)],
        );
        assert_refused(&p.check(), "claim");
    }
}

/// `CANONICAL_EQ` is what makes `equals` the identity, so the same claim holds under that bound.
#[test]
fn identity_comes_from_a_canonical_bound() {
    let text = generic(
        "HONEST[K: CANONICAL_EQ]",
        "  same(a: K, b: K): BOOL\n    require\n      eq: a.equals(b)\n    do\n      True\n    ensure\n      claim: a = b\n    end\n",
    );
    assert_ok(&Project::new("canonical", &[("honest.gdl", &text)]).check());
}

/// The trap in linking an instance. `SORTED`'s theory gives `K_less` the laws of `ORD`, so
/// assuming `K_less` is `BROKEN.less_than` while proving those laws for `BROKEN` would prove them
/// for anything. This order is empty, so `total` is false.
#[test]
fn an_order_cannot_prove_its_laws_through_its_own_use() {
    let broken = LEVEL
        .replace("LEVEL", "BROKEN")
        .replace("Current.rank > other.rank", "False")
        .replace(
            "specify {EQ}",
            "  pick(other: BROKEN): BROKEN\n    do\n      SORTED.smaller(Current, other)\n    end\n\nspecify {EQ}",
        );
    let p = Project::new("self_link", &[("sorted.gdl", SORTED), ("broken.gdl", &broken)]);
    assert_refused(&p.check(), "comparable");
}

/// A link is an assumption of the module that chose the instance, so it is no part of the
/// theory that a client of that module receives.
#[test]
fn a_link_stays_in_the_module_that_chose_it() {
    let middle = client(
        "  least(a: NAT, b: NAT): NAT\n    do\n      SORTED.smaller(a, b)\n    ensure\n      low: Result <= a\n    end\n",
    );
    let outer = "module OUTER

feature {ANY}

  twice(c: CLIENT, a: NAT): NAT
    do
      c.least(a, a)
    end

end
";
    let p = Project::new(
        "inherit",
        &[("sorted.gdl", SORTED), ("client.gdl", &middle), ("outer.gdl", outer)],
    );
    assert_ok(&p.check());
    assert!(p.generated("client.bpl").contains("_bx"), "the client links its instance");
    assert!(!p.generated("outer.bpl").contains("_bx"), "the link is not inherited");
}

#[test]
fn a_bound_is_checked_at_each_use() {
    let refused = [
        ("written", "f(s: SORTED[BYTES]): NAT\n    do\n      0\n    end\n"),
        ("result", "f: LIST[SORTED[F64]]\n    do\n      LIST.empty\n    end\n"),
        ("inferred_call", "f(s: BYTES): BYTES\n    do\n      SORTED.smaller(s, s)\n    end\n"),
        ("inferred_make", "f(s: BYTES): NAT\n    do\n      let x = SORTED.make(s)\n      0\n    end\n"),
        ("list", "f(s: LIST[NAT]): NAT\n    do\n      let x = SORTED.make(s)\n      0\n    end\n"),
        ("binder", "f(o: OPTION[BYTES]): NAT\n    do\n      match o\n        case Some(s) then\n          let x = SORTED.make(s)\n          0\n        case None then 0\n      end\n    end\n"),
        ("quantifier", "f(n: NAT): BOOL\n    do\n      True\n    ensure\n      all: forall s: SORTED[BYTES]. True\n    end\n"),
        ("lambda", "f(n: NAT): NAT\n    do\n      let g = fn(s: SORTED[BYTES]): NAT do 0 end\n      0\n    end\n"),
        ("routine_parameter", "f[A](a: A): NAT\n    do\n      let x = SORTED.make(a)\n      0\n    end\n"),
    ];
    for (name, routine) in refused {
        let text = client(&format!("  {routine}"));
        let p = Project::new(&format!("use_{name}"), &[("sorted.gdl", SORTED), ("client.gdl", &text)]);
        assert_refused(&p.check(), "E0317: unsatisfied_bound");
    }

    let unbounded = "module WRAP[A]\n\nrepresentation {ANY}\n  held: SORTED[A]\n\nend\n";
    let p = Project::new("unbounded", &[("sorted.gdl", SORTED), ("wrap.gdl", unbounded)]);
    assert_refused(&p.check(), "E0317: unsatisfied_bound");

    // A parameter's own bound is enough, and a `where` limits a claim, not its module.
    let bounded = "module WRAP[A: ORD]\n\nrepresentation {ANY}\n  held: SORTED[A]\n\nend\n";
    let text = client(
        "  f(s: STRING): OPTION[STRING]\n    do\n      Some(s)\n    end\n\n  g(n: NAT, b: BOOL, c: CHAR): NAT\n    do\n      let x = SORTED.make(n)\n      let y = SORTED.make(b)\n      let z = SORTED.make(c)\n      0\n    end\n",
    );
    let p = Project::new(
        "satisfied",
        &[("sorted.gdl", SORTED), ("wrap.gdl", bounded), ("client.gdl", &text)],
    );
    assert_ok(&p.check());
}

/// A constructor of a generic sum type names no routine, so only its inferred type shows the use.
#[test]
fn a_constructor_is_a_use() {
    let bag = "module BAG[K: ORD]\n\nrepresentation {ANY}\n  | Nothing\n  | One(k: K)\n\nend\n";
    let bad = client("  f(s: BYTES): NAT\n    do\n      let x = One(s)\n      0\n    end\n");
    let p = Project::new("ctor_bad", &[("bag.gdl", bag), ("client.gdl", &bad)]);
    assert_refused(&p.check(), "E0317: unsatisfied_bound");

    let good = client("  f(n: NAT): NAT\n    do\n      let x = One(n)\n      0\n    end\n");
    assert_ok(&Project::new("ctor_ok", &[("bag.gdl", bag), ("client.gdl", &good)]).check());
}

#[test]
fn a_type_parameter_has_only_the_methods_of_its_bound() {
    let less_on_eq = generic(
        "ONLY_EQ[K: EQ]",
        "  f(a: K, b: K): BOOL\n    do\n      a.less_than(b)\n    end\n",
    );
    let p = Project::new("method_eq", &[("only_eq.gdl", &less_on_eq)]);
    assert_refused(&p.check(), "E0318: method_not_in_bound");

    let unbounded = "module BARE[K]

representation {ANY}
  item: K

create {ANY}

  f(a: K, b: K): BOOL
    do
      a.equals(b)
    end

end
";
    let p = Project::new("method_bare", &[("bare.gdl", unbounded)]);
    assert_refused(&p.check(), "E0318: method_not_in_bound");
}

#[test]
fn a_bound_without_a_theory_is_refused() {
    let text = generic("ODD[K: NO_SUCH_CONCEPT]", "  f(a: K): K\n    do\n      a\n    end\n");
    let p = Project::new("no_concept", &[("odd.gdl", &text)]);
    assert_refused(&p.check(), "E0320: unsupported_bound");
}

#[test]
fn a_variant_without_a_measure_says_so() {
    let list = "module J

representation {ANY}
  | N
  | A(items: LIST[J])

feature {ANY}

  count(xs: LIST[J]): NAT
    variant
      xs
    do
      if xs.length = 0 then 0 else 1 + J.count(xs.tail) end
    end

end
";
    let p = Project::new("variant_list", &[("j.gdl", list)]);
    let out = p.check();
    assert_refused(&out, "E0319: variant_no_measure");
    assert!(!all(&out).contains("E0199"), "{}", all(&out));

    let law = "module T

representation {ANY}
  | L
  | B(next: T)

specify {ANY}

  law f(s: STRING, t: T)
    variant
      s
    do
      True
    ensure
      fine: True
    end

end
";
    let p = Project::new("variant_law", &[("t.gdl", law)]);
    assert_refused(&p.check(), "E0319: variant_no_measure");

    // A sum type of another module is measured in a routine, but not in a law.
    let foreign = law.replace("s: STRING", "s: OPTION[NAT]");
    let p = Project::new("variant_law_sum", &[("t.gdl", &foreign)]);
    assert_refused(&p.check(), "E0319: variant_no_measure");

    // A `LIST` is measured by its length, and a sum type by its size.
    let measured = list.replace("      xs\n    do", "      xs.length\n    do");
    let measured = measured.replace("1 + J.count(xs.tail)", "1");
    assert_ok(&Project::new("variant_ok", &[("j.gdl", &measured)]).check());
}

#[test]
fn equals_works_on_a_match_binder_and_a_quantified_variable() {
    let table = "module TABLE

representation {ANY}
  | Empty
  | Field(key: STRING, value: NAT, rest: TABLE)

feature {ANY}

  lookup(k: STRING): OPTION[NAT]
    variant
      Current
    do
      match Current
        case Empty then None
        case Field(key, value, rest) then
          if key.equals(k) then Some(value) else rest.lookup(k) end
      end
    end

  front(k: STRING): BOOL
    do
      match Current
        case Empty then False
        case Field(key, _, _) then key.equals(k)
      end
    ensure
      reflexive: forall s: STRING. s.equals(s)
    end

end
";
    assert_ok(&Project::new("binder", &[("table.gdl", table)]).check());

    let quantified = generic(
        "BELOW[K: ORD]",
        "  no_least(a: K): BOOL\n    do\n      True\n    ensure\n      none: forall x: K. not (x.less_than(a) and a.less_than(x))\n    end\n",
    );
    assert_ok(&Project::new("quantified", &[("below.gdl", &quantified)]).check());
}

#[test]
fn a_quantified_variable_ends_with_its_quantifier() {
    // `s` is a `STRING` under the quantifier, and the `NAT` parameter after it.
    let text = client(
        "  f(s: NAT): BOOL
    do
      True
    ensure
      both: (forall s: STRING. s.equals(s)) and s.equals(s)
    end
",
    );
    assert_ok(&Project::new("scope", &[("client.gdl", &text)]).check());
}

/// A generic over a generic: `CACHE`'s `X` is `SORTED`'s `K`, and a client fixes both.
#[test]
fn an_instance_reaches_through_a_generic() {
    let cache = "module CACHE[X: ORD]

representation {ANY}
  inner: SORTED[X]

create {ANY}

  pick(a: X, b: X): X
    do
      SORTED.smaller(a, b)
    ensure
      least: not a.less_than(Result)
    end

end
";
    let text = client(
        "  f(a: NAT, b: NAT): NAT\n    do\n      CACHE.pick(a, b)\n    ensure\n      low: Result <= a\n    end\n",
    );
    let p = Project::new(
        "nested",
        &[("sorted.gdl", SORTED), ("cache.gdl", cache), ("client.gdl", &text)],
    );
    assert_ok(&p.check());

    let lie = client(
        "  f(a: NAT, b: NAT): NAT\n    do\n      CACHE.pick(a, b)\n    ensure\n      claim: Result >= a\n    end\n",
    );
    let p = Project::new(
        "nested_lie",
        &[("sorted.gdl", SORTED), ("cache.gdl", cache), ("client.gdl", &lie)],
    );
    assert_refused(&p.check(), "claim");
}

#[test]
fn a_bound_crosses_a_package_boundary() {
    let root = std::env::temp_dir().join(format!("gardelc_bounds_{}_pkg", std::process::id()));
    drop(fs::remove_dir_all(&root));
    fs::create_dir_all(root.join("lib/src")).expect("lib");
    fs::write(root.join("lib/Gardel.toml"), "[project]\nname = \"lib\"\nversion = \"0.1.0\"\n")
        .expect("manifest");
    fs::write(root.join("lib/src/sorted.gdl"), SORTED).expect("source");
    fs::write(root.join("lib/src/level.gdl"), LEVEL).expect("source");

    let run = |name: &str, routine: &str| {
        let dir = root.join(name);
        fs::create_dir_all(dir.join("src")).expect("app");
        let manifest = format!(
            "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[dependencies]\nlib = {{ path = \"../lib\" }}\n"
        );
        fs::write(dir.join("Gardel.toml"), manifest).expect("manifest");
        fs::write(dir.join("src/client.gdl"), client(routine)).expect("source");
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&dir)
            .env("GARDEL_CACHE_DIR", root.join("cache"))
            .args(["--project", ".", "--strict"])
            .output()
            .expect("gardelc runs")
    };

    assert_ok(&run(
        "app",
        "  f(a: LEVEL, b: LEVEL): LEVEL\n    do\n      SORTED.smaller(a, b)\n    ensure\n      top: not a.less_than(Result)\n    end\n",
    ));
    let bad = run(
        "bad",
        "  f(s: BYTES): BYTES\n    do\n      SORTED.smaller(s, s)\n    end\n",
    );
    assert_refused(&bad, "E0317: unsatisfied_bound");
    drop(fs::remove_dir_all(&root));
}

#[test]
fn a_string_instance_of_an_eq_bound_runs() {
    let same = "module SAME[K: EQ]

representation {ANY}
  item: K

create {ANY}

  same(a: K, b: K): BOOL
    do
      a.equals(b)
    ensure
      said: Result = a.equals(b)
    end

end
";
    let main = "program MAIN

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        if SAME.same(\"ab\", \"ab\") and not SAME.same(\"ab\", \"ac\") then IO.exit(5) else IO.exit(6) end
      end
    end

end
";
    let text = client(
        "  f(s: STRING): BOOL\n    do\n      SAME.same(s, s)\n    ensure\n      holds: Result = s.equals(s)\n    end\n",
    );
    let p = Project::new(
        "string_eq",
        &[("same.gdl", same), ("client.gdl", &text), ("main.gdl", main)],
    );
    assert_ok(&p.check());
    assert_eq!(p.run_binary(), 5);
}

#[test]
fn a_record_holds_a_function_and_runs_it() {
    let table = "module TABLE

representation {ANY}
  routes: LIST[ROUTE]

create {ANY}

  empty: TABLE
    do
      { routes = LIST.empty }
    end

  find(rs: LIST[ROUTE], i: NAT, x: NAT): OPTION[NAT]
    variant
      rs.length
    do
      if rs.length = 0 then
        None
      else
        let r = rs.head
        if r.id = i then Some(r.handler(x)) else TABLE.find(rs.tail, i, x) end
      end
    end

feature {ANY}

  add(r: ROUTE): TABLE
    do
      { Current with routes = Current.routes.prepend(r) }
    end

  dispatch(i: NAT, x: NAT): OPTION[NAT]
    do
      TABLE.find(Current.routes, i, x)
    end

end
";
    let main = "program MAIN

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let a = ROUTE.make(1, fn(x: NAT): NAT do x +% 30 end)
      let k = 40
      let b = ROUTE.make(2, fn(x: NAT): NAT do x +% k end)
      let t = TABLE.empty.add(a).add(b)
      with IO
        match t.dispatch(1, 2)
          case Some(v) then IO.exit(v +% b.run(1))
          case None then IO.exit(9)
        end
      end
    end

end
";
    let p = Project::new(
        "fn_field",
        &[("route.gdl", ROUTE), ("table.gdl", table), ("main.gdl", main)],
    );
    assert_ok(&p.check());
    assert_eq!(p.run_binary(), 32 + 41);
}

#[test]
fn a_record_holding_a_function_has_no_equality() {
    let text = client(
        "  f(a: ROUTE, b: ROUTE): BOOL\n    do\n      a = b\n    end\n",
    );
    let p = Project::new("fn_eq", &[("route.gdl", ROUTE), ("client.gdl", &text)]);
    assert_refused(&p.check(), "E0311: non_canonical_equality");

    let list = client(
        "  f(a: LIST[ROUTE], b: LIST[ROUTE]): BOOL\n    do\n      a = b\n    end\n",
    );
    let p = Project::new("fn_list_eq", &[("route.gdl", ROUTE), ("client.gdl", &list)]);
    assert_refused(&p.check(), "E0311: non_canonical_equality");
}
