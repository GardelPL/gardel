//! Refined function types through the command line (Task F.12): a `require` is proven at each
//! application, an `ensure` is proven of what is passed and assumed where it is applied, and the
//! clauses are refused wherever nothing would enforce them.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, sources: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_refined_{}_{name}", std::process::id()));
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

/// The verifier refuses each condition in `tags`, and nothing else.
fn assert_refused_exactly(out: &Output, tags: &[&str]) {
    let text = all(out);
    assert!(!out.status.success(), "must fail:\n{text}");
    let errors: Vec<&str> = text.lines().filter(|l| l.contains("[ERROR]")).collect();
    for tag in tags {
        assert!(
            errors.iter().any(|l| l.contains(&format!("({tag})"))),
            "must refuse `{tag}`:\n{text}"
        );
    }
    assert_eq!(errors.len(), tags.len(), "only {tags:?} may fail:\n{text}");
}

fn functions(name: &str, routines: &str) -> String {
    format!("functions {name}\n\nfeature {{ANY}}\n\n{routines}\nend\n")
}

const APPLY: &str = "  apply_small(f: (x: NAT) -> NAT require small: x < 100 ensure bounded: Result <= x, k: NAT): NAT
    require
      k_small: k < 100
    do
      f(k)
    ensure
      le: Result <= k
    end

  two(f: (a: NAT, b: NAT) -> NAT require ordered: a <= b ensure below: Result <= b): NAT
    do
      f(1, 2)
    ensure
      at_most_two: Result <= 2
    end

  third: (x: NAT) -> NAT ensure le: Result <= x
    do
      fn(x: NAT): NAT do x / 3 end
    end
";

#[test]
fn a_refined_function_type_is_proven_where_it_is_passed_and_used_where_it_is_applied() {
    let client = functions(
        "CLIENT",
        "  halves: NAT
    do
      APPLY.apply_small(fn(x: NAT): NAT do x / 2 end, 50)
    ensure
      small: Result <= 50
    end

  shifted(c: NAT): NAT
    require
      c_small: c <= 3
    do
      APPLY.apply_small(fn(x: NAT): NAT do if x >= c then x - c else 0 end end, 9)
    end

  passes_on(g: (y: NAT) -> NAT require under: y < 200 ensure b: Result <= y): NAT
    do
      APPLY.apply_small(g, 3)
    end

  uses_its_require: NAT
    do
      APPLY.two(fn(a: NAT, b: NAT): NAT do b - a end)
    end

  uses_a_result(n: NAT): NAT
    do
      let g = APPLY.third
      g(n)
    ensure
      le: Result <= n
    end
",
    );
    let laws = "functions LAWS

specify {ANY}

  law below(f: (x: NAT) -> NAT ensure le: Result <= x, n: NAT)
    ensure
      ok: f(n) <= n
    end

end
";
    let p = Project::new(
        "accepted",
        &[("apply.gdl", &functions("APPLY", APPLY)), ("client.gdl", &client), ("laws.gdl", laws)],
    );
    assert_ok(&p.check());
}

#[test]
fn no_false_contract_gets_through_a_refined_function_type() {
    let client = functions(
        "CLIENT",
        "  breaks_ensure: NAT
    do
      APPLY.apply_small(fn(x: NAT): NAT do x +% 1 end, 5)
    end

  stronger_require(g: (y: NAT) -> NAT require tiny: y < 50 ensure b: Result <= y): NAT
    do
      APPLY.apply_small(g, 3)
    end

  claims_more(f: (x: NAT) -> NAT require small: x < 100 ensure bounded: Result <= x): NAT
    do
      f(5)
    ensure
      wrong: Result < 5
    end

  outside(f: (x: NAT) -> NAT require tight: x < 100): NAT
    do
      f(500)
    end

  in_a_lambda(f: (x: NAT) -> NAT require snug: x < 5): NAT
    do
      let g = fn(y: NAT): NAT do f(y) end
      0
    end

  to_a_plain_type(f: (x: NAT) -> NAT require cosy: x < 5, xs: LIST[NAT]): LIST[NAT]
    do
      LIST_OPS.map(xs, f)
    end

  in_an_option(f: (x: NAT) -> NAT require neat: x < 5): OPTION[NAT -> NAT]
    do
      Some(f)
    end

  grows: (x: NAT) -> NAT ensure shrinks: Result <= x
    do
      fn(x: NAT): NAT do x +% 1 end
    end
",
    );
    let p = Project::new("lies", &[("apply.gdl", &functions("APPLY", APPLY)), ("client.gdl", &client)]);
    assert_refused_exactly(
        &p.check(),
        // `g` keeps its `ensure` only below 50, so `f_bounded` fails too.
        &["f_bounded", "g_tiny", "f_bounded", "wrong", "tight", "snug", "f_cosy", "f_neat", "result_shrinks"],
    );
}

const ROUTE: &str = "module ROUTE

representation {ANY}
  id: NAT
  handler: (x: NAT) -> NAT ensure le: Result <= x

create {ANY}

  make(i: NAT, h: (x: NAT) -> NAT ensure le: Result <= x): ROUTE
    do
      { id = i, handler = h }
    end

feature {ANY}

  run(x: NAT): NAT
    do
      Current.handler(x)
    ensure
      le: Result <= x
    end

end
";

#[test]
fn a_record_field_keeps_its_contract_and_the_program_runs() {
    let table = "module TABLE

representation {ANY}
  routes: LIST[ROUTE]

create {ANY}

  find(rs: LIST[ROUTE], i: NAT, x: NAT): NAT
    variant
      rs.length
    do
      if rs.length = 0 then
        0
      else
        let r = rs.head
        if r.id = i then r.handler(x) else TABLE.find(rs.tail, i, x) end
      end
    ensure
      le: Result <= x
    end

end
";
    let twice = functions(
        "TWICE",
        "  twice(f: (x: NAT) -> NAT require positive: x > 0 ensure lower: Result < x, k: NAT): NAT
    require
      k_big: k > 1
    do
      let a = f(k)
      if a > 0 then f(a) else a end
    ensure
      lower: Result < k
    end
",
    );
    let main = "program MAIN

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let r = ROUTE.make(1, fn(x: NAT): NAT do x / 2 end)
      let t = TWICE.twice(fn(x: NAT): NAT do x - 1 end, 10)
      with IO
        IO.exit(r.run(40) +% t)
      end
    end

end
";
    let p = Project::new(
        "field",
        &[("route.gdl", ROUTE), ("table.gdl", table), ("twice.gdl", &twice), ("main.gdl", main)],
    );
    assert_ok(&p.check());
    assert_eq!(p.run_binary(), 20 + 8);
}

#[test]
fn a_record_is_built_only_from_a_function_that_keeps_the_contract() {
    let builds = "module BUILDS

representation {ANY}
  n: NAT

create {ANY}

  plain(h: NAT -> NAT): ROUTE
    do
      ROUTE.make(1, h)
    end

  grows: ROUTE
    do
      ROUTE.make(1, fn(x: NAT): NAT do x +% 2 end)
    end

end
";
    let loose = ROUTE.replace(
        "feature {ANY}",
        "  loose(i: NAT, h: NAT -> NAT): ROUTE
    do
      { id = i, handler = h }
    end

feature {ANY}",
    );
    let p = Project::new("record_lies", &[("route.gdl", &loose), ("builds.gdl", builds)]);
    assert_refused_exactly(&p.check(), &["handler_le", "h_le", "h_le"]);
}

#[test]
fn clauses_that_nothing_would_enforce_are_refused() {
    let cases: [(&str, &str, &str); 8] = [
        (
            "list_argument",
            "  a(fs: LIST[(x: NAT) -> NAT ensure le: Result <= x]): NAT\n    do\n      0\n    end\n",
            "E0321",
        ),
        (
            "let_type",
            "  a: NAT\n    do\n      let g: ((x: NAT) -> NAT ensure le: Result <= x) = fn(x: NAT): NAT do x end\n      g(1)\n    end\n",
            "E0321",
        ),
        (
            "lambda_parameter",
            "  a: NAT\n    do\n      let k = fn(g: (x: NAT) -> NAT ensure le: Result <= x): NAT do 0 end\n      0\n    end\n",
            "E0321",
        ),
        (
            "result_require",
            "  a: (x: NAT) -> NAT require s: x < 5\n    do\n      fn(x: NAT): NAT do x end\n    end\n",
            "E0321",
        ),
        (
            "bound_to_a_name",
            "  a(f: (x: NAT) -> NAT require s: x < 5): NAT\n    do\n      let g = f\n      g(1)\n    end\n",
            "E0322",
        ),
        (
            "returned",
            "  a(f: (x: NAT) -> NAT require s: x < 5): NAT -> NAT\n    do\n      f\n    end\n",
            "E0322",
        ),
        (
            "nested_function_type",
            "  a(f: (g: (y: NAT) -> NAT ensure le: Result <= y) -> NAT): NAT\n    do\n      0\n    end\n",
            "E0321",
        ),
        (
            "in_a_quantifier",
            "  a: BOOL\n    do\n      True\n    ensure\n      q: forall f: ((x: NAT) -> NAT ensure le: Result <= x). True\n    end\n",
            "E0321",
        ),
    ];
    for (name, routine, code) in cases {
        let p = Project::new(name, &[("p.gdl", &functions("P", routine))]);
        let out = p.check();
        assert!(!out.status.success(), "{name} must be refused:\n{}", all(&out));
        assert!(all(&out).contains(code), "{name} must be {code}:\n{}", all(&out));
    }
    let field = "module P\n\nrepresentation {ANY}\n  h: (x: NAT) -> NAT require s: x < 5\n\nend\n";
    let variant = "module Q\n\nrepresentation {ANY}\n  | A(h: (x: NAT) -> NAT ensure le: Result <= x)\n  | B\n\nend\n";
    for (name, text) in [("field_require", field), ("variant_field", variant)] {
        let p = Project::new(name, &[("p.gdl", text)]);
        let out = p.check();
        assert!(all(&out).contains("E0321"), "{name} must be E0321:\n{}", all(&out));
    }
}

/// A routine's name passed where a function is expected is that routine as a function value
/// (Task F.6), checked against a refined type exactly as the lambda `fn(x) do r(x) end` is.
#[test]
fn a_routine_name_is_a_function_value() {
    let client = functions(
        "CLIENT",
        "  halve(x: NAT): NAT
    do
      x / 2
    ensure
      le: Result <= x
    end

  bounded_halve(x: NAT): NAT
    require
      under: x < 100
    do
      x / 2
    ensure
      le: Result <= x
    end

  by_name: NAT
    do
      APPLY.apply_small(halve, 50)
    ensure
      small: Result <= 50
    end

  within_the_require: NAT
    do
      APPLY.apply_small(bounded_halve, 50)
    end

  shadowed(halve: NAT): NAT
    do
      halve / 3
    end

  mapped(xs: LIST[NAT]): LIST[NAT]
    do
      LIST_OPS.map(xs, halve)
    end
",
    );
    let main = "program MAIN

feature {ANY}

  quarter(x: NAT): NAT
    do
      x / 4
    ensure
      le: Result <= x
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let r = ROUTE.make(1, quarter)
      with IO
        IO.exit(r.run(40))
      end
    end

end
";
    let p = Project::new(
        "routine_values",
        &[("apply.gdl", &functions("APPLY", APPLY)), ("client.gdl", &client), ("route.gdl", ROUTE), ("main.gdl", main)],
    );
    assert_ok(&p.check());
    assert_eq!(p.run_binary(), 10);

    let lies = functions(
        "LIES",
        "  grow(x: NAT): NAT
    do
      x +% 1
    end

  tiny_only(x: NAT): NAT
    require
      tiny: x < 10
    do
      x
    ensure
      le: Result <= x
    end

  grows: NAT
    do
      APPLY.apply_small(grow, 5)
    end

  needs_more: NAT
    do
      APPLY.apply_small(tiny_only, 5)
    end
",
    );
    let p = Project::new("routine_value_lies", &[("apply.gdl", &functions("APPLY", APPLY)), ("lies.gdl", &lies)]);
    // `tiny_only` keeps its `ensure` only below 10, so `f_bounded` fails there too.
    assert_refused_exactly(&p.check(), &["f_bounded", "tiny", "f_bounded"]);
}
