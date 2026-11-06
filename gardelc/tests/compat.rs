//! `--compare-with` through the command line (Task E.7): what breaks a client of a package,
//! and that only a positive answer from the solver counts as compatible.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const CALC: &str = "functions CALC

feature {ANY}

  half(x: U64): U64
    require
      positive: x > 0
    do
      x / 2
    ensure
      small: Result <= x
    end

  twice(x: U64): U64
    require
      fits: x < 1000
    do
      x + x
    ensure
      grows: Result >= x
      exact: Result = x * 2
    end
end
";

const SHAPE: &str = "module SHAPE

representation {ANY}
  | Dot
  | Line(len: U64)

feature {ANY}

  width: U64
    do
      match Current
        case Dot then 0
        case Line(n) then n
      end
    end
end
";

const LABEL: &str = "functions LABEL

feature {ANY}

  name(s: SHAPE): STRING
    do
      \"x\"
    ensure
      fixed: Result.equals(\"x\")
      wide: s.width >= 0
    end

  find(o: OPTION[U64]): U64
    require
      has: o.is_some
    do
      o.get
    ensure
      val: Result = o.get
    end
end
";

const STACK: &str = include_str!("../../examples/bounded_stack.gdl");

struct World {
    dir: PathBuf,
}

impl World {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_compat_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).expect("temporary directory");
        Self { dir }
    }

    /// A version of the package `acme.lib`.
    fn version(&self, version: &str, sources: &[(&str, &str)]) {
        self.named("acme.lib", version, sources);
    }

    fn named(&self, package: &str, version: &str, sources: &[(&str, &str)]) {
        let root = self.dir.join(version);
        fs::create_dir_all(root.join("src")).expect("source directory");
        let manifest = format!("[project]\nname = \"{package}\"\nversion = \"0.1.0\"\n");
        fs::write(root.join("Gardel.toml"), manifest).expect("manifest");
        for (file, text) in sources {
            fs::write(root.join("src").join(file), text).expect("source file");
        }
    }

    fn compare_env(&self, extra: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_gardelc"));
        cmd.arg("--compare-with").arg("../old").args(extra).current_dir(self.dir.join("new"));
        cmd.env("GARDEL_CACHE_DIR", self.dir.join("cache"));
        env.iter().for_each(|(k, v)| {
            cmd.env(k, v);
        });
        cmd.output().expect("run gardelc")
    }

    fn compare(&self) -> Output {
        self.compare_env(&[], &[])
    }
}

impl Drop for World {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn assert_compatible(out: &Output) {
    let t = text(out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains("[compare]") && t.contains(": 0 breaking"), "{t}");
}

fn assert_breaking(out: &Output, needle: &str) {
    let t = text(out);
    assert_eq!(out.status.code(), Some(1), "{t}");
    assert!(t.contains("breaking_change") && t.contains(needle), "wanted `{needle}` in {t}");
}

/// `(old, new)` run through one package.
fn changed(name: &str, old: &str, new: &str) -> (World, Output) {
    let world = World::new(name);
    world.version("old", &[("calc.gdl", old)]);
    world.version("new", &[("calc.gdl", new)]);
    let out = world.compare();
    (world, out)
}

fn calc_with(from: &str, to: &str) -> String {
    assert!(CALC.contains(from), "{from}");
    CALC.replace(from, to)
}

#[test]
fn a_stronger_require_is_breaking() {
    let (_w, out) = changed("stronger", CALC, &calc_with("positive: x > 0", "positive: x > 1"));
    assert_breaking(&out, "CALC.half: `require positive`");
}

#[test]
fn a_body_change_is_not_breaking() {
    let (_w, out) = changed("body", CALC, &calc_with("x / 2\n", "(x / 2) + 0\n"));
    assert_compatible(&out);
    assert!(text(&out).contains("0 changed clauses"), "no clause changed, so no solver: {}", text(&out));
}

#[test]
fn a_weaker_require_is_proved_compatible() {
    let (_w, out) = changed("weaker", CALC, &calc_with("fits: x < 1000", "fits: x < 2000"));
    assert_compatible(&out);
    assert!(text(&out).contains("1 changed clauses"), "{}", text(&out));
}

#[test]
fn a_weaker_ensure_is_breaking() {
    let (_w, out) = changed("weaker_ensure", CALC, &calc_with("      exact: Result = x * 2\n", ""));
    assert_breaking(&out, "`ensure exact`");
}

#[test]
fn a_stronger_ensure_is_proved_compatible() {
    let new = calc_with("      grows: Result >= x\n      exact: Result = x * 2\n", "      exact: Result = x * 2\n");
    let (_w, out) = changed("stronger_ensure", CALC, &new);
    assert_compatible(&out);
}

#[test]
fn an_old_require_is_available_to_an_old_ensure() {
    // The new `ensure` holds only under the `require`, so the proof must use it.
    let old = "functions CALC\n\nfeature {ANY}\n\n  f(x: U64): U64\n    require\n      big: x > 10\n    do\n      x\n    ensure\n      a: Result > 10\n    end\nend\n";
    let new = old.replace("a: Result > 10", "a: Result = x");
    let (_w, out) = changed("old_require", old, &new);
    assert_compatible(&out);
}

#[test]
fn a_removed_routine_is_breaking_and_a_new_one_is_not() {
    let new = calc_with("  twice(x: U64): U64", "  double(x: U64): U64");
    let (_w, out) = changed("removed", CALC, &new);
    assert_breaking(&out, "`twice` is removed or no longer public");
    assert!(text(&out).contains("[compare] added: CALC.double"), "{}", text(&out));
}

#[test]
fn a_routine_that_stops_being_public_is_breaking() {
    let new = format!(
        "{}\nfeature {{NONE}}\n\n  twice(x: U64): U64\n    do\n      x\n    end\nend\n",
        "functions CALC\n\nfeature {ANY}\n\n  half(x: U64): U64\n    require\n      positive: x > 0\n    do\n      x / 2\n    ensure\n      small: Result <= x\n    end\n"
    );
    let (_w, out) = changed("private", CALC, &new);
    assert_breaking(&out, "`twice` is removed or no longer public");
}

#[test]
fn a_changed_signature_is_breaking() {
    let (_w, out) = changed("signature", CALC, &calc_with("half(x: U64): U64", "half(x: I64): U64"));
    // `x > 0` still parses, but the routine is another one.
    assert_breaking(&out, "the signature of `half` changed");
}

#[test]
fn a_renamed_parameter_is_not_breaking() {
    let new = CALC.replace(
        "half(x: U64): U64\n    require\n      positive: x > 0\n    do\n      x / 2\n    ensure\n      small: Result <= x",
        "half(y: U64): U64\n    require\n      positive: y > 0\n    do\n      y / 2\n    ensure\n      small: Result <= y",
    );
    assert_ne!(new, CALC);
    let (_w, out) = changed("renamed", CALC, &new);
    assert_compatible(&out);
}

/// A caller passes arguments by position, so swapped names are a different contract.
#[test]
fn swapped_parameters_are_compared_by_position() {
    let old = "functions CALC\n\nfeature {ANY}\n\n  f(a: U64, b: U64): U64\n    require\n      r: a > b\n    do\n      a\n    end\nend\n";
    let new = old.replace("f(a: U64, b: U64)", "f(b: U64, a: U64)");
    let (_w, out) = changed("swapped", old, &new);
    assert_breaking(&out, "`require r`");
}

/// The new name of a parameter may be a binder of the old clause. Renamed in place, the old
/// `forall z: U64. z >= x` would turn into `forall z: U64. z >= z`, which is true.
#[test]
fn a_renamed_parameter_is_not_captured_by_a_binder() {
    let old = "functions CALC

feature {ANY}

  f(x: U64): U64
    do
      x
    ensure
      e: forall z: U64. z >= x
    end
end
";
    let new = "functions CALC

feature {ANY}

  f(z: U64): U64
    do
      z
    end
end
";
    let (_w, out) = changed("capture", old, new);
    assert_breaking(&out, "`ensure e`");
}

#[test]
fn a_changed_public_representation_is_breaking() {
    let world = World::new("repr");
    world.version("old", &[("shape.gdl", SHAPE)]);
    let new = SHAPE.replace("| Line(len: U64)\n", "| Line(len: U64)\n  | Box\n").replace(
        "case Line(n) then n\n",
        "case Line(n) then n\n        case Box then 0\n",
    );
    world.version("new", &[("shape.gdl", &new)]);
    assert_breaking(&world.compare(), "the public `representation` changed");
}

#[test]
fn a_hidden_representation_can_change() {
    let old = STACK.replace("module BOUNDED_STACK[A]", "module BOUNDED_STACK[A]");
    let new = STACK.replace("size = Current.size + 1,", "size = 1 + Current.size,");
    assert_ne!(old, new);
    let world = World::new("hidden");
    world.version("old", &[("bounded_stack.gdl", &old)]);
    world.version("new", &[("bounded_stack.gdl", &new)]);
    assert_compatible(&world.compare());
}

#[test]
fn generic_modules_are_compared() {
    let world = World::new("generic");
    world.version("old", &[("bounded_stack.gdl", STACK)]);
    let same_meaning = STACK.replace(
        "not_full: Current.count < Current.capacity",
        "not_full: Current.count + 1 <= Current.capacity",
    );
    world.version("new", &[("bounded_stack.gdl", &same_meaning)]);
    let out = world.compare();
    assert_compatible(&out);
    assert!(text(&out).contains("1 changed clauses"), "{}", text(&out));

    let dropped = STACK.replace("      capacity_unchanged: Result.capacity = Current.capacity\n", "");
    world.version("new", &[("bounded_stack.gdl", &dropped)]);
    assert_breaking(&world.compare(), "`ensure capacity_unchanged`");
}

#[test]
fn clauses_may_use_strings_prelude_types_and_other_modules() {
    let world = World::new("mixed");
    world.version("old", &[("shape.gdl", SHAPE), ("label.gdl", LABEL)]);
    let new = LABEL.replace("has: o.is_some", "has: not o.is_none").replace(
        "wide: s.width >= 0",
        "wide: s.width + 0 >= 0",
    );
    world.version("new", &[("shape.gdl", SHAPE), ("label.gdl", &new)]);
    // Each file changed goes through the solver with the theories it needs.
    let out = world.compare();
    let t = text(&out);
    assert!(!t.contains("Boogie refused"), "{t}");

    let lost = LABEL.replace("fixed: Result.equals(\"x\")", "fixed: Result.equals(\"y\") or Result.equals(\"x\")");
    world.version("new", &[("shape.gdl", SHAPE), ("label.gdl", &lost)]);
    let out = world.compare();
    assert!(!text(&out).contains("Boogie refused"), "{}", text(&out));
    assert_breaking(&out, "`ensure fixed`");
}

#[test]
fn a_removed_module_is_breaking() {
    let world = World::new("module");
    world.version("old", &[("shape.gdl", SHAPE), ("calc.gdl", CALC)]);
    world.version("new", &[("calc.gdl", CALC)]);
    assert_breaking(&world.compare(), "SHAPE: the module is removed");
}

#[test]
fn a_renamed_package_is_breaking() {
    let world = World::new("package");
    world.version("old", &[("calc.gdl", CALC)]);
    world.named("acme.other", "new", &[("calc.gdl", CALC)]);
    assert_breaking(&world.compare(), "the package was named `acme.lib`");
}

const HELPED: &str = "functions CALC

feature {ANY}

  h(x: U64): U64
    do
      7
    end

  f(x: U64): U64
    require
      r: h(x) > 0
    do
      x
    end
end
";

/// Nothing a package states may help the comparison. Here the clause reaches `h`, so a false
/// `ensure` of `h` would prove the stronger `require` of `f` if the solver could see it.
#[test]
fn a_false_contract_makes_nothing_compatible() {
    let new = HELPED
        .replace("r: h(x) > 0", "r: h(x) > 1")
        .replace("      7\n    end", "      assume bad: False end\n      7\n    ensure\n      absurd: False\n    end");
    assert_ne!(new, HELPED);
    let (_w, out) = changed("false", HELPED, &new);
    assert_breaking(&out, "`require r`");
}

/// The price of that: a true implication that needs a law of the package is unproven here.
#[test]
fn an_implication_that_needs_a_law_is_reported() {
    let law = "\nspecify {ANY}\n\n  law seven(x: U64)\n    trigger\n      h(x)\n    do\n      True\n    ensure\n      e: h(x) > 1\n    end\nend\n";
    let contract = HELPED.replace("      7\n    end", "      7\n    ensure\n      s: Result = 7\n    end");
    let new = contract.replace("r: h(x) > 0", "r: h(x) > 1").replace("    end\nend\n", &format!("    end\n{law}"));
    assert_ne!(new, contract);
    let (_w, out) = changed("needs_law", &contract, &new);
    assert_breaking(&out, "`require r`");
}

/// Numbers are in range for a real call, so a clause may rely on it.
#[test]
fn machine_ranges_are_known() {
    let old = "functions CALC\n\nfeature {ANY}\n\n  f(x: U64): U64\n    do\n      x\n    ensure\n      e: Result >= 0\n    end\nend\n";
    let new = old
        .replace("    do\n", "    require\n      r: x >= 0\n    do\n")
        .replace("    ensure\n      e: Result >= 0\n", "");
    let (_w, out) = changed("ranges", old, &new);
    assert_compatible(&out);
}

/// A routine exported to a friend is not part of what a client of the package sees.
#[test]
fn a_routine_for_a_friend_may_go() {
    let friend = "functions FRIEND\n\nfeature {ANY}\n\n  one: U64\n    do\n      1\n    end\nend\n";
    let open = "functions CALC\n\nfeature {ANY}\n\n  f(x: U64): U64\n    do\n      x\n    end\nend\n";
    let old = open.replace("end\nend\n", "end\n\nfeature {FRIEND}\n\n  g(x: U64): U64\n    do\n      x\n    end\nend\n");
    assert_ne!(old, open);
    let world = World::new("friend");
    world.version("old", &[("calc.gdl", &old), ("friend.gdl", friend)]);
    world.version("new", &[("calc.gdl", open), ("friend.gdl", friend)]);
    assert_compatible(&world.compare());
}

/// A clause that uses what is gone needs no solver, and does not spoil the rest of the file.
#[test]
fn a_clause_on_a_removed_routine_says_so() {
    let old = "functions CALC\n\nfeature {ANY}\n\n  h(x: U64): U64\n    do\n      x\n    end\n\n  f(x: U64): U64\n    do\n      x\n    ensure\n      e: h(Result) >= 0\n    end\n\n  g(x: U64): U64\n    require\n      r: x < 10\n    do\n      x\n    end\nend\n";
    let new = "functions CALC\n\nfeature {ANY}\n\n  f(x: U64): U64\n    do\n      x\n    end\n\n  g(x: U64): U64\n    require\n      r: x < 20\n    do\n      x\n    end\nend\n";
    let (_w, out) = changed("gone", old, new);
    assert_breaking(&out, "uses `h`, which the new version does not offer");
    let t = text(&out);
    assert!(t.contains("1 changed clauses") && !t.contains("Boogie refused"), "{t}");
}

#[test]
fn a_public_invariant_is_compared() {
    let module = "module POS\n\nrepresentation {ANY}\n  value: U64\n\ncreate {ANY}\n\n  make: POS\n    do\n      { value = 1 }\n    end\n\ninvariant\n  positive: Current.value > 0\nend\n";
    let (_w, out) = changed("invariant_kept", module, &module.replace("Current.value > 0", "Current.value >= 1"));
    assert_compatible(&out);
    let (_w, out) = changed("invariant_gone", module, &module.replace("invariant\n  positive: Current.value > 0\n", ""));
    assert_breaking(&out, "`invariant positive`");
}

#[test]
fn a_concept_that_is_dropped_is_breaking() {
    let plain = "module TAG\n\nrepresentation {ANY}\n  | One\n  | Two\n\nend\n";
    let old = plain.replace("module TAG", "module TAG satisfies CANONICAL_EQ");
    let (_w, out) = changed("concept", &old, plain);
    assert_breaking(&out, "no longer satisfies `CANONICAL_EQ`");
}

/// A law is a promise too, and a client may use it as one.
#[test]
fn a_public_law_is_compared() {
    let module = "functions CALC\n\nfeature {ANY}\n\n  f(x: U64): U64\n    do\n      x\n    ensure\n      same: Result = x\n    end\n\nspecify {ANY}\n\n  law l(x: U64)\n    require\n      r: x > 0\n    do\n      True\n    ensure\n      e: f(x) > 0\n    end\nend\n";
    let (_w, out) = changed("law_kept", module, &module.replace("r: x > 0", "r: x > 0 or x > 1"));
    assert_compatible(&out);
    let (_w, out) = changed("law_narrow", module, &module.replace("r: x > 0", "r: x > 1"));
    assert_breaking(&out, "`require r`");
    let gone = module.replace("  law l(x: U64)\n    require\n      r: x > 0\n    do\n      True\n    ensure\n      e: f(x) > 0\n    end\n", "");
    let (_w, out) = changed("law_gone", module, &gone);
    assert_breaking(&out, "law `l` is removed");
}

/// A new name is not a breaking change, but a client using that name bare from another
/// package gets `E0105`, so the report must say so, and `--strict` must not make it fatal.
#[test]
fn a_new_public_name_is_a_warning() {
    let world = World::new("new_name");
    world.version("old", &[("calc.gdl", CALC)]);
    world.version("new", &[("calc.gdl", CALC), ("shape.gdl", SHAPE), ("sized.gdl", SIZED)]);
    for flags in [&[][..], &["--strict"][..]] {
        let out = world.compare_env(flags, &[]);
        assert_compatible(&out);
        let t = text(&out);
        assert!(t.contains("Module `SHAPE` is new.") && t.contains("Concept `SIZED` is new."), "{t}");
        assert!(t.contains("new_public_name") && t.contains("must now qualify it"), "{t}");
    }
}

#[test]
fn a_representation_that_becomes_public_is_a_warning() {
    let hidden = SHAPE.replace("representation {ANY}", "representation {NONE}");
    let world = World::new("repr_public");
    world.version("old", &[("shape.gdl", &hidden)]);
    world.version("new", &[("shape.gdl", SHAPE)]);
    let out = world.compare();
    assert_compatible(&out);
    assert!(text(&out).contains("is now public, with its constructors"), "{}", text(&out));
}

/// Nothing new, nothing to say.
#[test]
fn a_new_routine_gives_no_warning() {
    let new = calc_with("  twice(x: U64): U64", "  extra(x: U64): U64\n    do\n      x\n    end\n\n  twice(x: U64): U64");
    let (_w, out) = changed("no_warning", CALC, &new);
    assert_compatible(&out);
    assert!(!text(&out).contains("new_public_name"), "{}", text(&out));
}

const FN_LIB: &str = "functions LIB\n\nfeature {ANY}\n\n  one: U64\n    do\n      1\n    end\nend\n";
const FN_BASE: &str = "functions BASE\n\nfeature {ANY}\n\n  one: U64\n    do\n      1\n    end\nend\n";

fn depend_on(world: &World, version: &str, entry: &str) {
    let path = world.dir.join(version).join("Gardel.toml");
    let mut manifest = fs::read_to_string(&path).expect("manifest");
    manifest.push_str(&format!("\n[dependencies]\n{entry}\n"));
    fs::write(path, manifest).expect("manifest");
}

/// A build holds one package of a name, so a client with its own copy of a new dependency of
/// this package no longer builds, even with no use of it.
#[test]
fn a_new_dependency_is_breaking() {
    let world = World::new("new_dep");
    world.named("acme.base", "base", &[("base.gdl", FN_BASE)]);
    world.version("old", &[("lib.gdl", FN_LIB)]);
    world.version("new", &[("lib.gdl", FN_LIB)]);
    depend_on(&world, "new", "\"acme.base\" = { path = \"../base\" }");
    assert_breaking(&world.compare(), "it now depends on `acme.base`");
}

#[test]
fn a_changed_dependency_version_is_breaking() {
    let world = World::new("dep_version");
    world.named("acme.base", "base", &[("base.gdl", FN_BASE)]);
    world.named("acme.base", "base2", &[("base.gdl", FN_BASE)]);
    let moved = fs::read_to_string(world.dir.join("base2/Gardel.toml")).expect("manifest");
    fs::write(world.dir.join("base2/Gardel.toml"), moved.replace("0.1.0", "0.2.0")).expect("manifest");
    world.version("old", &[("lib.gdl", FN_LIB)]);
    world.version("new", &[("lib.gdl", FN_LIB)]);
    depend_on(&world, "old", "\"acme.base\" = { path = \"../base\" }");
    depend_on(&world, "new", "\"acme.base\" = { path = \"../base2\" }");
    assert_breaking(&world.compare(), "`acme.base` went from 0.1.0 to 0.2.0");
}

#[test]
fn dropping_a_dependency_or_keeping_one_is_not_breaking() {
    let world = World::new("dep_same");
    world.named("acme.base", "base", &[("base.gdl", FN_BASE)]);
    world.version("old", &[("lib.gdl", FN_LIB)]);
    world.version("new", &[("lib.gdl", FN_LIB)]);
    depend_on(&world, "old", "\"acme.base\" = { path = \"../base\" }");
    assert_compatible(&world.compare());
    depend_on(&world, "new", "\"acme.base\" = { path = \"../base\" }");
    assert_compatible(&world.compare());
}

/// A clause Boogie cannot read, as its constructor is gone, says nothing about the others.
#[test]
fn an_unreadable_clause_does_not_answer_for_the_others() {
    let old = "module H\n\nrepresentation {ANY}\n  | Dot\n  | Line(len: U64)\n\nfeature {ANY}\n\n  f(x: U64): U64\n    require\n      r: x < 10\n    do\n      x\n    end\n\n  g(s: H): U64\n    do\n      1\n    ensure\n      e: (s = Dot) implies (Result = 1)\n    end\nend\n";
    let new = "module H\n\nrepresentation {ANY}\n  | Line(len: U64)\n\nfeature {ANY}\n\n  f(x: U64): U64\n    require\n      r: x < 20\n    do\n      x\n    end\n\n  g(s: H): U64\n    do\n      1\n    end\nend\n";
    let (_w, out) = changed("unreadable", old, new);
    assert_breaking(&out, "`ensure e`");
    let t = text(&out);
    assert!(!t.contains("`require r`"), "the weaker `require` is compatible: {t}");
    assert!(t.contains("1 changed clauses"), "{t}");
}

/// A body-only change, but a client under `--strict` no longer builds (decision 6).
#[test]
fn a_new_assume_is_breaking() {
    let holed = calc_with("      x / 2\n", "      assume hole: x > 0 end\n      x / 2\n");
    let (_w, out) = changed("assume", CALC, &holed);
    assert_breaking(&out, "CALC.half: `assume hole` is new");

    // In a routine no client can call, it is refused all the same.
    let private = format!(
        "{}\nfeature {{NONE}}\n\n  inner(x: U64): U64\n    do\n      assume deep: x > 0 end\n      x\n    end\nend\n",
        CALC.trim_end().strip_suffix("end").expect("a closing end")
    );
    let (_w, out) = changed("assume_private", CALC, &private);
    assert_breaking(&out, "`assume deep` is new");

    // One that was already there has already been seen by every client.
    let (_w, out) = changed("assume_old", &holed, &holed.replace("x / 2\n", "(x / 2) + 0\n"));
    assert_compatible(&out);
}

/// `--strict` covers the whole build, so an `assume` can arrive through a dependency.
#[test]
fn a_new_assume_in_a_dependency_is_breaking() {
    let world = World::new("assume_dep");
    let lib = "functions LIB\n\nfeature {ANY}\n\n  one: U64\n    do\n      1\n    end\nend\n";
    world.named("acme.dep", "dep_old", &[("dep.gdl", "functions DEP\n\nfeature {ANY}\n\n  f(x: U64): U64\n    do\n      x\n    end\nend\n")]);
    world.named("acme.dep", "dep_new", &[("dep.gdl", "functions DEP\n\nfeature {ANY}\n\n  f(x: U64): U64\n    do\n      assume hole: x > 0 end\n      x\n    end\nend\n")]);
    world.version("old", &[("lib.gdl", lib)]);
    world.version("new", &[("lib.gdl", lib)]);
    for (version, dep) in [("old", "dep_old"), ("new", "dep_new")] {
        let path = world.dir.join(version).join("Gardel.toml");
        let mut manifest = fs::read_to_string(&path).expect("manifest");
        manifest.push_str(&format!("\n[dependencies]\n\"acme.dep\" = {{ path = \"../{dep}\" }}\n"));
        fs::write(path, manifest).expect("manifest");
    }
    assert_breaking(&world.compare(), "`assume hole` is new");
}

const SIZED: &str = "concept SIZED

  size: U64

  doubled: U64
    do
      Current.size
    ensure
      same: Result = Current.size
    end

  law sane(a: Current)
    ensure
      fine: a.size >= 0
    end

end
";

/// `(old, new)` of a concept, in a package that also has a module.
fn concept_changed(name: &str, new: &str) -> (World, Output) {
    let world = World::new(&format!("concept_{name}"));
    world.version("old", &[("sized.gdl", SIZED), ("calc.gdl", CALC)]);
    world.version("new", &[("sized.gdl", new), ("calc.gdl", CALC)]);
    let out = world.compare();
    (world, out)
}

#[test]
fn a_removed_concept_is_breaking() {
    let world = World::new("concept_gone");
    world.version("old", &[("sized.gdl", SIZED), ("calc.gdl", CALC)]);
    world.version("new", &[("calc.gdl", CALC)]);
    assert_breaking(&world.compare(), "SIZED: the concept is removed");
}

#[test]
fn a_changed_concept_is_breaking_in_either_direction() {
    let cases = [
        ("sig", SIZED.replace("  size: U64\n", "  size: I64\n"), "the signature of `size` changed"),
        ("extra", SIZED.replace("  size: U64\n", "  size: U64\n  extra: U64\n"), "`extra` is new"),
        ("params", SIZED.replace("concept SIZED", "concept SIZED[T]"), "its type parameters changed"),
        ("law", SIZED.replace("a.size >= 0", "a.size >= 1"), "law `sane` changed"),
        ("weak", SIZED.replace("  law sane(a: Current)\n    ensure\n      fine: a.size >= 0\n    end\n\n", ""), "law `sane` is removed"),
        ("more", SIZED.replace("  law sane(a", "  law other(a: Current)\n    ensure\n      more: a.size >= 0\n    end\n\n  law sane(a"), "law `other` is new"),
        ("def", SIZED.replace("same: Result = Current.size", "same: Result >= Current.size"), "the default routine `doubled` changed"),
        ("refine", SIZED.replace("concept SIZED", "concept SIZED refines EQ"), "what it refines changed"),
    ];
    for (name, new, needle) in &cases {
        assert_ne!(new, SIZED, "{name}");
        let (_w, out) = concept_changed(name, new);
        assert_breaking(&out, needle);
    }
}

#[test]
fn a_default_body_and_a_new_concept_are_not_breaking() {
    let (_w, out) = concept_changed("default_body", &SIZED.replace("      Current.size\n    ensure", "      Current.size + 0\n    ensure"));
    assert_compatible(&out);

    let world = World::new("concept_new");
    world.version("old", &[("calc.gdl", CALC)]);
    world.version("new", &[("sized.gdl", SIZED), ("calc.gdl", CALC)]);
    let out = world.compare();
    assert_compatible(&out);
    assert!(text(&out).contains("[compare] added: SIZED"), "{}", text(&out));
}

#[test]
fn a_new_version_that_does_not_verify_is_not_compared() {
    let (_w, out) = changed("unverified", CALC, &calc_with("small: Result <= x", "small: Result > x"));
    let t = text(&out);
    assert_eq!(out.status.code(), Some(1), "{t}");
    assert!(!t.contains("[compare]"), "{t}");
}

#[test]
fn the_flag_needs_two_packages() {
    let world = World::new("flags");
    world.version("new", &[("calc.gdl", CALC)]);
    let out = world.compare();
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("--compare-with"), "{}", text(&out));

    world.version("old", &[("calc.gdl", CALC)]);
    for flag in ["--stdlib", "--bounded-check", "--inspect", "--run"] {
        let out = world.compare_env(&[flag], &[]);
        assert!(text(&out).contains(&format!("cannot be combined with {flag}")), "{}", text(&out));
        assert_eq!(out.status.code(), Some(1));
    }

    let file = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .args(["--compare-with", "old", "new/src/calc.gdl"])
        .current_dir(&world.dir)
        .output()
        .expect("run gardelc");
    assert_eq!(file.status.code(), Some(1));
    assert!(text(&file).contains("needs a project"), "{}", text(&file));

    let bare = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .arg("--compare-with")
        .current_dir(world.dir.join("new"))
        .output()
        .expect("run gardelc");
    assert_eq!(bare.status.code(), Some(1));
    assert!(text(&bare).contains("expected the directory"), "{}", text(&bare));
}

#[test]
fn breaking_changes_are_in_the_json_report() {
    let world = World::new("json");
    world.version("old", &[("calc.gdl", CALC)]);
    world.version("new", &[("calc.gdl", &calc_with("positive: x > 0", "positive: x > 1"))]);
    let out = world.compare_env(&["--json-diagnostics"], &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout.contains("breaking_change") && stdout.contains("Contract Compatibility"), "{stdout}");
}
