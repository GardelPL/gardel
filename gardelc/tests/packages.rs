//! Packages through the command line (Task E.4): what a manifest puts in a build, which module
//! a name means there, and that only proven contracts cross a package boundary.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Output};

const PROJECT: &str = "[project]\nname = \"{}\"\nversion = \"0.1.0\"\n";

struct World {
    dir: PathBuf,
}

impl World {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_pkg_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).expect("temporary directory");
        let world = Self { dir };
        files.iter().for_each(|(path, text)| world.write(path, text));
        world
    }

    fn write(&self, path: &str, text: &str) {
        let full = self.dir.join(path);
        fs::create_dir_all(full.parent().expect("parent")).expect("directory");
        fs::write(full, text).expect("source file");
    }

    /// A package with a manifest listing `deps`, each `(name, entry)`.
    fn package(&self, name: &str, deps: &[(&str, &str)], sources: &[(&str, &str)]) {
        let mut manifest = PROJECT.replace("{}", name);
        if !deps.is_empty() {
            manifest.push_str("\n[dependencies]\n");
            for (dep, entry) in deps {
                manifest.push_str(&format!("\"{dep}\" = {entry}\n"));
            }
        }
        self.write(&format!("{name}/Gardel.toml"), &manifest);
        for (file, text) in sources {
            self.write(&format!("{name}/src/{file}"), text);
        }
    }

    fn run(&self, package: &str, args: &[&str]) -> Output {
        let dir = self.dir.join(package);
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&dir)
            .env("GARDEL_CACHE_DIR", self.dir.join("cache"))
            .args(args)
            .output()
            .expect("gardelc runs")
    }

    fn build_file(&self, package: &str, file: &str) -> String {
        fs::read_to_string(self.dir.join(package).join("build").join(file)).expect("build output")
    }

    fn build_dir(&self, package: &str) -> PathBuf {
        self.dir.join(package).join("build")
    }
}

impl Drop for World {
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

fn tally(out: &Output) -> (usize, usize) {
    let text = all(out);
    let line = text.lines().find(|l| l.contains("[cache]")).expect("a cache line");
    let numbers: Vec<usize> = line.split_whitespace().filter_map(|w| w.parse().ok()).collect();
    (numbers[0], numbers[1])
}

fn parser(value: u64) -> String {
    format!(
        "module PARSER

representation {{ANY}}
  | Leaf(value: U64)
  | Pair(left: U64, right: U64)

feature {{ANY}}

  size: U64
    do
      match Current
        case Leaf(v) then {value}
        case Pair(l, r) then {value} + 1
      end
    ensure
      fixed: Result >= {value}
    end

end
"
    )
}

fn api(module: &str, value: u64) -> String {
    format!(
        "functions {module}

feature {{ANY}}

  answer: U64
    do
      Leaf(7).size
    ensure
      known: Result >= {value}
    end

  label: STRING
    do
      if Leaf(7).size = {value} then \"{module} {value}\" else \"wrong\" end
    end

end
"
    )
}

const APP: &str = "program MAIN_APP

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(ALPHA_API.label)
        IO.print_line(BETA_API.label)
        IO.exit(0)
      end
    end

end
";

fn two_libraries(world: &World, beta_entry: &str) {
    world.package("alpha.parse", &[], &[("parser.gdl", &parser(10)), ("alpha_api.gdl", &api("ALPHA_API", 10))]);
    world.package("beta-parse", &[], &[("parser.gdl", &parser(20)), ("beta_api.gdl", &api("BETA_API", 20))]);
    world.package(
        "app",
        &[("alpha.parse", "{ path = \"../alpha.parse\" }"), ("beta-parse", beta_entry)],
        &[("main_app.gdl", APP)],
    );
}

/// The Roadmap's acceptance: one module name and one constructor name in two packages.
#[test]
fn two_packages_with_one_name_verify_link_and_run() {
    let world = World::new("two", &[]);
    two_libraries(&world, "{ path = \"../beta-parse\" }");
    let out = world.run("app", &["--strict"]);
    assert_ok(&out);
    assert_eq!(tally(&out), (5, 0));
    let exe = world.build_dir("app").join("main_app_bin.exe");
    let run = Command::new(exe).output().expect("the program runs");
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace('\r', ""), "ALPHA_API 10\nBETA_API 20\n");
}

#[test]
fn each_package_keeps_its_own_proofs() {
    let world = World::new("own", &[]);
    two_libraries(&world, "{ path = \"../beta-parse\" }");
    // True of alpha's `PARSER`, false of beta's: proving it would mean a mix-up of the two.
    let false_of_beta = api("BETA_API", 20).replace("known: Result >= 20", "known: Result = 10");
    world.write("beta-parse/src/beta_api.gdl", &false_of_beta);
    let out = world.run("app", &["--strict"]);
    assert_refused(&out, "`known`");
}

#[test]
fn an_alias_reaches_no_symbol_and_no_proof() {
    let world = World::new("alias", &[]);
    two_libraries(&world, "{ path = \"../beta-parse\", as = \"FIRST_NAME\" }");
    assert_ok(&world.run("app", &["--strict"]));
    let files: Vec<String> = fs::read_dir(world.build_dir("app"))
        .expect("build directory")
        .map(|e| e.expect("entry").file_name().to_string_lossy().to_string())
        .filter(|f| f.ends_with(".bpl") || f.ends_with(".c"))
        .collect();
    assert_eq!(files.len(), 10, "{files:?}");
    let first: Vec<String> = files.iter().map(|f| world.build_file("app", f)).collect();
    two_libraries(&world, "{ path = \"../beta-parse\", as = \"SECOND_NAME\" }");
    let out = world.run("app", &["--strict"]);
    assert_ok(&out);
    assert_eq!(tally(&out), (0, 5), "a new alias must prove nothing again");
    for (file, before) in files.iter().zip(&first) {
        assert_eq!(*before, world.build_file("app", file), "{file}");
    }
    assert!(first.iter().all(|text| !text.contains("FIRST_NAME")));
}

const FIVE: &str = "functions DEP

feature {ANY}

  five: U64
    do
      5
    end

end
";

const USES_FIVE: &str = "functions CLIENT

feature {ANY}

  use: U64
    do
      DEP.five
    ensure
      exact: Result = 5
    end

end
";

/// Finding 2 of the Roadmap, across a real manifest.
#[test]
fn only_a_contract_crosses_a_package() {
    let world = World::new("contract", &[]);
    world.package("dep", &[], &[("dep.gdl", FIVE)]);
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("client.gdl", USES_FIVE)]);
    assert_refused(&world.run("root", &["--strict"]), "`exact`");

    let promised = FIVE.replace("      5\n    end", "      5\n    ensure\n      five: Result = 5\n    end");
    world.write("dep/src/dep.gdl", &promised);
    assert_ok(&world.run("root", &["--strict"]));

    // A body change inside the contract re-proves the package, and not its client.
    world.write("dep/src/dep.gdl", &promised.replace("      5\n", "      2 + 3\n"));
    let out = world.run("root", &["--strict"]);
    assert_ok(&out);
    assert_eq!(tally(&out), (1, 1));
}

#[test]
fn a_false_dependency_fails_its_client() {
    let world = World::new("false", &[]);
    let lie = FIVE.replace("      5\n    end", "      5\n    ensure\n      lie: Result = 6\n    end");
    world.package("dep", &[], &[("dep.gdl", &lie)]);
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("client.gdl", USES_FIVE)]);
    let out = world.run("root", &[]);
    assert_refused(&out, "`lie`");
    assert!(all(&out).contains("dep.gdl"), "must name the file: {}", all(&out));
}

#[test]
fn strict_covers_a_dependency_and_a_warm_cache_changes_nothing() {
    let world = World::new("assume", &[]);
    let hole = FIVE.replace(
        "      5\n    end",
        "      assume hole: False end\n      5\n    ensure\n      five: Result = 5\n    end",
    );
    world.package("dep", &[], &[("dep.gdl", &hole)]);
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("client.gdl", USES_FIVE)]);
    let weak = world.run("root", &[]);
    assert!(weak.status.success(), "{}", all(&weak));
    assert!(all(&weak).contains("Dependency `dep`"), "{}", all(&weak));
    for _ in 0..2 {
        assert_refused(&world.run("root", &["--strict"]), "Dependency `dep`");
    }
}

#[test]
fn a_project_module_wins_over_a_dependency() {
    let world = World::new("nearest", &[]);
    world.package("dep", &[], &[("parser.gdl", &parser(10))]);
    let client = "functions CLIENT

feature {ANY}

  use: U64
    do
      PARSER.seven
    ensure
      own: Result = 7
    end

end
";
    let own = "functions PARSER

feature {ANY}

  seven: U64
    do
      7
    ensure
      seven: Result = 7
    end

end
";
    world.package("root", &[], &[("client.gdl", client), ("parser.gdl", own)]);
    assert_ok(&world.run("root", &["--strict"]));

    // Adding a dependency that declares the name must not change what local code means.
    let bpl = world.build_file("root", "client.bpl");
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[]);
    let out = world.run("root", &["--strict"]);
    assert_ok(&out);
    assert_eq!(bpl, world.build_file("root", "client.bpl"));
}

#[test]
fn two_dependencies_with_one_name_are_ambiguous() {
    let world = World::new("ambiguous", &[]);
    world.package("one", &[], &[("parser.gdl", &parser(1))]);
    world.package("two", &[], &[("parser.gdl", &parser(2))]);
    let client = "functions CLIENT

feature {ANY}

  use(p: PARSER): U64
    do
      p.size
    end

end
";
    world.package(
        "root",
        &[("one", "{ path = \"../one\" }"), ("two", "{ path = \"../two\", as = \"OTHER\" }")],
        &[("client.gdl", client)],
    );
    let out = world.run("root", &[]);
    assert_refused(&out, "E0105: ambiguous_module_reference");
    let text = all(&out);
    assert!(text.contains("`one` (alias `ONE`)") && text.contains("`two` (alias `OTHER`)"), "{text}");
    assert!(text.contains("client.gdl:5"), "must point at the use: {text}");
}

#[test]
fn a_constructor_of_two_dependencies_is_ambiguous() {
    let world = World::new("ctor", &[]);
    let a = "module AA\n\nrepresentation {ANY}\n  | Leaf(v: U64)\n\nend\n";
    let b = "module BB\n\nrepresentation {ANY}\n  | Leaf(v: U64)\n\nend\n";
    world.package("one", &[], &[("aa.gdl", a)]);
    world.package("two", &[], &[("bb.gdl", b)]);
    let client = "functions CLIENT

feature {ANY}

  use: U64
    do
      match Leaf(1)
        case Leaf(v) then v
      end
    end

end
";
    world.package(
        "root",
        &[("one", "{ path = \"../one\" }"), ("two", "{ path = \"../two\" }")],
        &[("client.gdl", client)],
    );
    assert_refused(&world.run("root", &[]), "E0112: ambiguous_constructor");
}

/// Decision 5: a package's dependency adds no name to its client.
#[test]
fn a_transitive_dependency_adds_no_names() {
    let world = World::new("transitive", &[]);
    world.package("low", &[], &[("dep.gdl", FIVE.replace("      5\n    end", "      5\n    ensure\n      five: Result = 5\n    end").as_str())]);
    let mid = "functions MID

feature {ANY}

  five: U64
    do
      DEP.five
    ensure
      five: Result = 5
    end

end
";
    world.package("mid", &[("low", "{ path = \"../low\" }")], &[("mid.gdl", mid)]);
    let through = "functions CLIENT

feature {ANY}

  use: U64
    do
      MID.five
    ensure
      exact: Result = 5
    end

end
";
    world.package("root", &[("mid", "{ path = \"../mid\" }")], &[("client.gdl", through)]);
    let out = world.run("root", &["--strict"]);
    assert_ok(&out);
    assert_eq!(tally(&out), (3, 0));

    world.write("root/src/client.gdl", USES_FIVE);
    assert_refused(&world.run("root", &[]), "E0106: unknown_module");
}

/// One package reached twice is one package.
#[test]
fn a_diamond_is_one_package() {
    let world = World::new("diamond", &[]);
    let base = FIVE.replace("      5\n    end", "      5\n    ensure\n      five: Result = 5\n    end");
    world.package("base", &[], &[("dep.gdl", &base)]);
    let side = |name: &str| {
        format!("functions {name}\n\nfeature {{ANY}}\n\n  five: U64\n    do\n      DEP.five\n    ensure\n      five: Result = 5\n    end\n\nend\n")
    };
    world.package("left", &[("base", "{ path = \"../base\" }")], &[("left_side.gdl", &side("LEFT_SIDE"))]);
    world.package("right", &[("base", "{ path = \"../base\" }")], &[("right_side.gdl", &side("RIGHT_SIDE"))]);
    let client = "functions CLIENT

feature {ANY}

  use: U64
    do
      LEFT_SIDE.five + RIGHT_SIDE.five
    ensure
      ten: Result = 10
    end

end
";
    world.package(
        "root",
        &[("left", "{ path = \"../left\" }"), ("right", "{ path = \"../right\" }")],
        &[("client.gdl", client)],
    );
    let out = world.run("root", &["--strict"]);
    assert_ok(&out);
    assert_eq!(tally(&out), (4, 0));
}

fn manifest_error(entry: &str, text: &str) {
    let world = World::new(&format!("manifest_{}", text.len()), &[]);
    world.package("dep", &[], &[("dep.gdl", FIVE)]);
    world.package("other", &[], &[("other.gdl", &FIVE.replace("DEP", "OTHER"))]);
    world.package("root", &[], &[("client.gdl", USES_FIVE)]);
    world.write(
        "root/Gardel.toml",
        &format!("{}\n[dependencies]\n{entry}\n", PROJECT.replace("{}", "root")),
    );
    assert_refused(&world.run("root", &[]), text);
}

#[test]
fn a_manifest_states_its_dependencies_correctly() {
    manifest_error("dep = \"1.0\"", ":6: dependency 'dep' must be an inline table");
    manifest_error("dep = { }", ":6: dependency 'dep' has no 'path'");
    manifest_error("dep = { path = \"../dep\", git = \"x\" }", ":6: unknown key 'git'");
    manifest_error("Dep = { path = \"../dep\" }", ":6: invalid package name 'Dep'");
    manifest_error("dep = { path = \"../dep\", as = \"GARDEL\" }", ":6: invalid alias 'GARDEL'");
    manifest_error("dep = { path = \"../dep\", as = \"lower\" }", ":6: invalid alias 'lower'");
    manifest_error("dep = { path = \"../none\" }", ":6: dependency 'dep': no Gardel.toml");
    manifest_error("dep = { path = \"../other\" }", ":6: dependency 'dep': ");
    manifest_error(
        "dep = { path = \"../dep\", as = \"X\" }\nother = { path = \"../other\", as = \"X\" }",
        ":7: dependencies 'dep' and 'other' both have the alias 'X'",
    );
}

#[test]
fn a_dependency_cycle_is_refused() {
    let world = World::new("cycle", &[]);
    world.package("aa", &[("bb", "{ path = \"../bb\" }")], &[("dep.gdl", FIVE)]);
    world.package("bb", &[("aa", "{ path = \"../aa\" }")], &[("other.gdl", &FIVE.replace("DEP", "OTHER"))]);
    assert_refused(&world.run("aa", &[]), "dependency cycle: aa -> bb -> aa");
}

#[test]
fn two_packages_of_one_name_are_refused() {
    let world = World::new("twice", &[]);
    world.package("dep", &[], &[("dep.gdl", FIVE)]);
    world.write("elsewhere/dep/Gardel.toml", &PROJECT.replace("{}", "dep"));
    world.write("elsewhere/dep/src/other.gdl", &FIVE.replace("DEP", "OTHER"));
    world.package("left", &[("dep", "{ path = \"../dep\" }")], &[("left_side.gdl", &FIVE.replace("DEP", "LEFT_SIDE"))]);
    world.package("right", &[("dep", "{ path = \"../elsewhere/dep\" }")], &[("right_side.gdl", &FIVE.replace("DEP", "RIGHT_SIDE"))]);
    world.package(
        "root",
        &[("left", "{ path = \"../left\" }"), ("right", "{ path = \"../right\" }")],
        &[("client.gdl", USES_FIVE)],
    );
    assert_refused(&world.run("root", &[]), "two packages are named 'dep'");

    // The project is a package too.
    world.package("self", &[("root", "{ path = \"../same\" }")], &[("client.gdl", USES_FIVE)]);
    world.write("same/Gardel.toml", &PROJECT.replace("{}", "root"));
    world.write("same/src/dep.gdl", FIVE);
    world.package("root", &[("root", "{ path = \"../same\" }")], &[]);
    assert_refused(&world.run("root", &[]), "two packages are named 'root'");
}

const WITH_ENTRY: &str = "program TOOL

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(\"the tool\")
        IO.exit(3)
      end
    end

end
";

#[test]
fn a_dependency_entry_point_is_not_a_root_and_is_not_exported() {
    let world = World::new("entry", &[]);
    let dep = FIVE.replace("      5\n    end", "      5\n    ensure\n      five: Result = 5\n    end");
    world.package("dep", &[], &[("dep.gdl", &dep), ("tool.gdl", WITH_ENTRY)]);
    let root = "program HOME

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(if DEP.five = 5 then \"home\" else \"no\" end)
        IO.exit(0)
      end
    end

end
";
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("home.gdl", root)]);
    assert_ok(&world.run("root", &["--strict"]));
    let run = Command::new(world.build_dir("root").join("home_bin.exe")).output().expect("runs");
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace('\r', ""), "home\n");
    assert!(!world.build_dir("root").join("pk3_dep_tool_bin.exe").exists());

    // A library with no entry point of its own builds no binary, whatever its dependency has.
    fs::remove_file(world.dir.join("root/src/home.gdl")).expect("remove");
    drop(fs::remove_dir_all(world.build_dir("root")));
    world.write("root/src/client.gdl", &FIVE.replace("DEP", "CLIENT"));
    assert_ok(&world.run("root", &["--strict"]));
    assert!(!world.build_dir("root").join("pk3_dep_tool_bin.exe").exists());

    let names_it = "functions CLIENT

feature {ANY}

  use: U64
    do
      let t = TOOL.main(SEQ.empty)
      1
    end

end
";
    world.write("root/src/client.gdl", names_it);
    assert_refused(&world.run("root", &[]), "E0104: entry_point_reference");
}

/// Decision 7: `{M}` names a module of the same package, so a module of one bare name in
/// another package is not `M`.
#[test]
fn an_export_list_stops_at_the_package() {
    let world = World::new("export", &[]);
    let friends = "group { FRIEND, SECRET }

functions FRIEND

feature {ANY}

  two: U64
    do
      SECRET.one + 1
    end

end

functions SECRET

feature {FRIEND}

  one: U64
    do
      1
    end

end
";
    world.package("dep", &[], &[("friends.gdl", friends)]);
    let same_name = "functions FRIEND

feature {ANY}

  steal: U64
    do
      SECRET.one
    end

end
";
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("friend.gdl", same_name)]);
    assert_refused(&world.run("root", &[]), "E0113: hidden_member");

    let names_other = "functions CLIENT

feature {SECRET}

  open: U64
    do
      1
    end

end
";
    world.write("root/src/friend.gdl", names_other.replace("CLIENT", "FRIEND").as_str());
    assert_refused(&world.run("root", &[]), "E0114: export_outside_package");

    // Nor can an export list reach the prelude. A concept is not a module, and stays allowed.
    world.write("root/src/friend.gdl", names_other.replace("CLIENT", "FRIEND").replace("{SECRET}", "{SEQ}").as_str());
    assert_refused(&world.run("root", &[]), "E0114: export_outside_package");
}

#[test]
fn hidden_things_stay_hidden_from_a_client() {
    let world = World::new("hidden", &[]);
    let dep = "module BOX

representation {NONE}
  | Filled(v: U64)

create {ANY}

  make(v: U64): BOX
    do
      Filled(v)
    end

feature {NONE}

  secret: U64
    do
      1
    end

end
";
    world.package("dep", &[], &[("box.gdl", dep)]);
    let forge = "-- Filled is not ours to build.
functions CLIENT

feature {ANY}

  forge: BOX
    do
      Filled(1)
    end

end
";
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("client.gdl", forge)]);
    let out = world.run("root", &[]);
    assert_refused(&out, "E0113: hidden_member");
    // `Filled` has an internal name by now, and is still found at its use in this routine.
    assert!(all(&out).contains("client.gdl:8:7"), "{}", all(&out));
    let call = forge.replace("forge: BOX", "peek: U64").replace("Filled(1)", "BOX.make(1).secret");
    world.write("root/src/client.gdl", &call);
    assert_refused(&world.run("root", &[]), "E0113: hidden_member");
}

/// Decision 8: `gardelc` builds in `Shutdown` and `Stop`, and a dependency's own constructors
/// of those names are other things.
#[test]
fn a_dependency_cannot_interfere_with_the_built_in_names() {
    let world = World::new("builtin", &[]);
    let dep = "module SIGNAL

representation {ANY}
  | Shutdown
  | Print(c: U64)
  | Stop

create {ANY}

  halt: SIGNAL
    do
      Stop
    end

feature {ANY}

  code: U64
    do
      match Current
        case Shutdown then 1
        case Print(c) then c
        case Stop then 3
      end
    end

end
";
    world.package("dep", &[], &[("signal.gdl", dep)]);
    let reactive = "reactive program LOOP

feature {ANY}

  init(args: SEQ[STRING]): STEP[U64]
    do
      Continue(SIGNAL.halt.code, LIST.empty)
    end

  step(s: U64, e: EVENT): STEP[U64]
    do
      match e
        case _ then
          Continue(s, LIST.empty)
      end
    end

end
";
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("loop.gdl", reactive)]);
    // `shutdown_must_stop` is about the prelude's `EVENT` and `STEP`, whatever `dep` declares.
    assert_refused(&world.run("root", &[]), "shutdown_must_stop");
}

#[test]
fn a_type_parameter_of_one_name_is_not_the_dependency() {
    let world = World::new("scope", &[]);
    let one = "functions AA\n\nfeature {ANY}\n\n  x: U64\n    do\n      1\n    end\n\nend\n";
    world.package("one", &[], &[("aa.gdl", one)]);
    world.package("two", &[], &[("aa.gdl", one)]);
    let client = "module HOLDER[AA]

representation {ANY}
  | Held(item: AA)

feature {ANY}

  item_of: AA
    do
      match Current
        case Held(i) then i
      end
    end

end
";
    world.package(
        "root",
        &[("one", "{ path = \"../one\" }"), ("two", "{ path = \"../two\" }")],
        &[("holder.gdl", client)],
    );
    assert_ok(&world.run("root", &["--strict"]));
}

#[test]
fn a_generic_module_crosses_a_package() {
    let world = World::new("generic", &[]);
    let stack = "module STACK[A]

representation {ANY}
  | Bottom
  | Push(top: A, rest: STACK[A])

create {ANY}

  empty: STACK[A]
    do
      Bottom
    end

feature {ANY}

  pushed(x: A): STACK[A]
    do
      Push(x, Current)
    end

end
";
    world.package("dep", &[], &[("stack.gdl", stack)]);
    let app = "program USES_STACK

feature {ANY}

  top_of(s: STACK[U64]): U64
    do
      match s
        case Push(top, _) then top
        case Bottom then 0
      end
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(if top_of(STACK.empty.pushed(2)) = 2 then \"top is two\" else \"wrong\" end)
        IO.exit(0)
      end
    end

end
";
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("uses_stack.gdl", app)]);
    assert_ok(&world.run("root", &["--strict"]));
    let run = Command::new(world.build_dir("root").join("uses_stack_bin.exe")).output().expect("runs");
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace('\r', ""), "top is two\n");
}

/// A concept is a name of its package, so two packages may each have a `SIZED`, and a client
/// states what one means by implementing it.
#[test]
fn a_concept_belongs_to_its_package() {
    let world = World::new("concept", &[]);
    let concept = |ret: &str| {
        format!("concept SIZED\n\n  width: {ret}\n\n  law stable(a: Current)\n    ensure\n      same: a.width = a.width\n    end\n\nend\n")
    };
    let block = "module BLOCK satisfies SIZED

representation {ANY}
  | Block(w: U64)

feature {ANY}

  width: U64
    do
      8
    end

specify {SIZED}

  law stable(a: BLOCK)
    do
      True
    ensure
      same: a.width = a.width
    end

end
";
    let tile = |ret: &str| {
        format!("module TILE satisfies SIZED\n\nrepresentation {{ANY}}\n  | Tile(w: U64)\n\nfeature {{ANY}}\n\n  width: {ret}\n    do\n      1\n    end\n\nspecify {{SIZED}}\n\n  law stable(a: TILE)\n    do\n      True\n    ensure\n      same: a.width = a.width\n    end\n\nend\n")
    };
    world.package("dep", &[], &[("sized.gdl", &concept("U64")), ("block.gdl", block)]);
    world.package("other", &[], &[("sized.gdl", &concept("NAT")), ("plank.gdl", &block.replace("BLOCK", "PLANK").replace("Block", "Plank").replace("width: U64", "width: NAT"))]);
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("tile.gdl", &tile("U64"))]);
    assert_ok(&world.run("root", &["--strict"]));

    // Only the concept of the dependency it lists: the other package adds no name.
    world.package("mid", &[("other", "{ path = \"../other\" }")], &[("mid.gdl", &FIVE.replace("DEP", "MID"))]);
    world.package("root", &[("dep", "{ path = \"../dep\" }"), ("mid", "{ path = \"../mid\" }")], &[]);
    assert_ok(&world.run("root", &["--strict"]));

    // With both in reach the bare name is ambiguous.
    world.package("root", &[("dep", "{ path = \"../dep\" }"), ("other", "{ path = \"../other\" }")], &[]);
    assert_refused(&world.run("root", &[]), "E0105: ambiguous_module_reference");
}

/// Decision 7: an entry point is not exported, so it cannot make another package's name
/// ambiguous.
#[test]
fn an_entry_point_provides_no_name() {
    let world = World::new("entry_name", &[]);
    let tool = WITH_ENTRY.replace("TOOL", "DEP");
    let promised = FIVE.replace("      5\n    end", "      5\n    ensure\n      five: Result = 5\n    end");
    world.package("tool", &[], &[("dep.gdl", &tool)]);
    world.package("dep", &[], &[("dep.gdl", &promised)]);
    world.package(
        "root",
        &[("tool", "{ path = \"../tool\" }"), ("dep", "{ path = \"../dep\" }")],
        &[("client.gdl", USES_FIVE)],
    );
    assert_ok(&world.run("root", &["--strict"]));
}

#[test]
fn verify_deps_proves_the_prelude_too() {
    let world = World::new("verify_deps", &[]);
    let promised = FIVE.replace("      5\n    end", "      5\n    ensure\n      five: Result = 5\n    end");
    world.package("dep", &[], &[("dep.gdl", &promised)]);
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("client.gdl", USES_FIVE)]);
    assert_ok(&world.run("root", &["--strict"]));
    let out = world.run("root", &["--strict", "--verify-deps"]);
    assert_ok(&out);
    assert_eq!(tally(&out), (2, 0), "a dependency is proven again, whatever the cache holds");
    assert!(all(&out).contains("Standard Prelude `gardel.core` proven"), "{}", all(&out));
}

/// The real second package: a service built on `ghttp`, whose modules it sees only as contracts.
#[test]
fn a_service_on_ghttp_verifies_and_answers() {
    let world = World::new("ghttp", &[]);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&world.dir)
        .env("GARDEL_CACHE_DIR", world.dir.join("cache"))
        .args(["--project", &root.join("examples/hello_api").to_string_lossy(), "--strict"])
        .output()
        .expect("gardelc runs");
    assert_ok(&out);
    assert!(all(&out).contains("ghttp::HTTP_PARSER"), "{}", all(&out));

    // The port is the example's own. If something else holds it, the proof above still stands.
    if std::net::TcpListener::bind(("127.0.0.1", 8090)).is_err() {
        return;
    }
    let mut server = Command::new(world.dir.join("build/hello_api_bin.exe")).spawn().expect("the service starts");
    let mut answer = String::new();
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Ok(mut socket) = std::net::TcpStream::connect(("127.0.0.1", 8090)) {
            socket.write_all(b"GET /hello HTTP/1.1\r\nHost: test\r\n\r\n").expect("request");
            socket.read_to_string(&mut answer).expect("response");
            break;
        }
    }
    drop(server.kill());
    drop(server.wait());
    assert_eq!(answer, "HTTP/1.1 200\r\nContent-Length: 20\r\n\r\nhello from a package");
}

#[test]
fn a_name_of_an_internal_form_cannot_be_declared() {
    let world = World::new("reserved", &[]);
    let fake = "functions PK3_DEP_DEP\n\nfeature {ANY}\n\n  x: U64\n    do\n      1\n    end\n\nend\n";
    world.package("root", &[], &[("pk3_dep_dep.gdl", fake)]);
    assert_refused(&world.run("root", &[]), "E0115: reserved_name");
}

#[test]
fn a_diagnostic_speaks_of_the_source_not_of_internal_names() {
    let world = World::new("readable", &[]);
    let bad = "functions BAD

feature {ANY}

  f(p: NOPE): U64
    do
      1
    end

end
";
    world.package("dep", &[], &[("bad.gdl", bad)]);
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("client.gdl", &FIVE.replace("DEP", "CLIENT"))]);
    let out = world.run("root", &[]);
    assert_refused(&out, "E0107: unknown_type");
    let text = all(&out);
    assert!(!text.contains("PK3_"), "{text}");
    assert!(text.contains("dep::BAD") && text.contains("bad.gdl:5"), "{text}");
}

#[test]
fn a_file_of_a_dependency_is_named_after_its_module() {
    let world = World::new("w0201", &[]);
    world.package("dep", &[], &[("dep.gdl", FIVE), ("wrong.gdl", &FIVE.replace("DEP", "RIGHT"))]);
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("client.gdl", &FIVE.replace("DEP", "CLIENT"))]);
    let out = world.run("root", &[]);
    assert!(out.status.success(), "{}", all(&out));
    let text = all(&out);
    assert!(text.contains("it should be named `right.gdl`") && !text.contains("dep.gdl`; "), "{text}");
}

#[test]
fn a_dependency_stays_out_of_inspect() {
    let world = World::new("inspect", &[]);
    let dep = "module BOX\n\nrepresentation {ANY}\n  | Filled(v: U64)\n\nend\n";
    world.package("dep", &[], &[("box.gdl", dep)]);
    let client = "functions CLIENT

feature {ANY}

  open(b: BOX): U64
    do
      match b
        case Filled(v) then v
      end
    end

end
";
    world.package("root", &[("dep", "{ path = \"../dep\" }")], &[("client.gdl", client)]);
    let out = world.run("root", &["--inspect"]);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{}", all(&out));
    assert!(text.contains("functions CLIENT") && text.contains("dep::BOX"), "{text}");
    assert!(!text.contains("module dep::BOX") && !text.contains("PK3_"), "{text}");
}

