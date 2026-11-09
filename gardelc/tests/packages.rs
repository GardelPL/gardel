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
    // Shown if the test then fails: a tool with no version makes every proof run again.
    text.lines().filter(|l| l.contains("[cache] off")).for_each(|l| println!("{l}"));
    let line = text.lines().find(|l| l.contains("verified now")).expect("a cache line");
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

/// `x.code` was the identity on every receiver, as `CHAR`'s is, so a false `ensure` verified.
/// Each name here belongs to a preamble theory too, and in a module it means that module's own
/// routine: the false claim is refuted, and the true one proven from the routine's body.
#[test]
fn a_routine_named_like_a_built_in_means_that_routine() {
    let module = |routine: &str, extra: &str, claim: &str| {
        format!(
            "module COLOR

representation {{ANY}}
  | Red
  | Green

feature {{ANY}}

  {routine}: COLOR
    do
      Green
    end

  claim(x: COLOR{extra}): U64
    do
      0
    ensure
      stated: {claim}
    end

end
"
        )
    };
    for (routine, extra, call) in [
        ("code", "", "code"),
        ("first", "", "first"),
        ("head", "", "head"),
        ("length", "", "length"),
        ("append(y: COLOR)", ", y: COLOR", "append(y)"),
        ("at(i: U64)", ", i: U64", "at(i)"),
        ("apply(y: COLOR)", ", y: COLOR", "apply(y)"),
    ] {
        let world = World::new("built_in_name", &[]);
        world.package("lie", &[], &[("color.gdl", &module(routine, extra, &format!("x.{call} = x")))]);
        let lie = world.run("lie", &["--strict"]);
        assert!(!lie.status.success(), "`{call}`: {}", all(&lie));
        assert!(!all(&lie).contains("E0199"), "`{call}` must be refuted, not untranslated: {}", all(&lie));
        world.package("truth", &[], &[("color.gdl", &module(routine, extra, &format!("Red.{call} = Green")))]);
        assert_ok(&world.run("truth", &["--strict"]));
    }
}

/// A routine of another type keeps `CHAR.code` a number, and `Result.code` reads the routine.
#[test]
fn a_routine_called_code_works_beside_char_code() {
    let world = World::new("code_beside_char", &[]);
    let signal = "module SIGNAL

representation {ANY}
  | Quiet
  | Loud(c: U64)

feature {ANY}

  code: U64
    do
      match Current
        case Quiet then 1
        case Loud(c) then c
      end
    end

  quiet: SIGNAL
    do
      Quiet
    ensure
      one: Result.code = 1
    end

  read_it(s: SIGNAL): U64
    do
      s.code
    end

  of_letter: U64
    do
      'a'.code
    ensure
      letter: Result = 97
    end

end
";
    world.package("root", &[], &[("signal.gdl", signal)]);
    assert_ok(&world.run("root", &["--strict"]));
    // Bend has no types here, so it must not take `Result.code` for `Result`.
    assert!(world.build_file("root", "signal.bend").contains("code(s)"), "the routine is called");
}

/// A declared type is checked, where nothing used to read it.
#[test]
fn a_let_type_must_match_its_value() {
    let body = |statement: &str| {
        format!(
            "module LETS

feature {{ANY}}

  size_of(n: NAT, u: U64, s: STRING): U64
    do
      {statement}
      1
    end

end
"
        )
    };
    for statement in [
        "let x: BOOL = 3",
        "let x: U64 = True",
        "let x: U64 = n",
        "let x: NAT = u",
        "let x: U64 = s",
        "let x: U64 = 18446744073709551616",
        "let x: I64 = 9223372036854775808",
        "let x: U64 = -1",
    ] {
        let world = World::new("let_mismatch", &[]);
        world.package("root", &[], &[("lets.gdl", &body(statement))]);
        assert_refused(&world.run("root", &[]), "E0116: let_type_mismatch");
    }
    for statement in ["let x: U64 = u", "let x: U64 = 18446744073709551615", "let x: I64 = -1", "let x: NAT = 7", "let x: STRING = s"] {
        let world = World::new("let_match", &[]);
        world.package("root", &[], &[("lets.gdl", &body(statement))]);
        assert_ok(&world.run("root", &["--strict"]));
    }
    let world = World::new("let_unknown", &[]);
    world.package("root", &[], &[("lets.gdl", &body("let x: NO_SUCH = 1"))]);
    assert_refused(&world.run("root", &[]), "E0107: unknown_type");
}

/// A declared type settles what the value alone leaves open, here across a package boundary,
/// so it has to be resolved like any other type.
#[test]
fn a_let_type_settles_a_generic_value_of_a_dependency() {
    let world = World::new("let_type", &[]);
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
      let s: STACK[U64] = STACK.empty
      with IO
        IO.print_line(if top_of(s.pushed(2)) = 2 then \"top is two\" else \"wrong\" end)
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
    // Each part is its own write, with a pause, so the server sees it as its own read.
    let ask = |parts: &[&str]| -> String {
        let mut socket = std::net::TcpStream::connect(("127.0.0.1", 8090)).expect("connect");
        socket.set_nodelay(true).expect("nodelay");
        for part in parts {
            socket.write_all(part.as_bytes()).expect("request");
            socket.flush().expect("flush");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let mut answer = String::new();
        socket.read_to_string(&mut answer).expect("response");
        answer
    };
    let up = (0..50).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        std::net::TcpStream::connect(("127.0.0.1", 8090)).is_ok()
    });
    let answers = if up {
        vec![
            ask(&["GET /hello HTTP/1.1\r\nHost: test\r\n\r\n"]),
            // Finding 1 of the libraries plan: a request cut across reads was answered as `GET /`.
            ask(&["GET /he", "llo HTTP/1.1\r\nHost: test\r\n\r\n"]),
            ask(&["GET /nope", " HTTP/1.1\r\nHost: test\r\n\r\n"]),
            // Finding 2: a body that does not end in a newline arrives.
            ask(&["POST /echo HTTP/1.1\r\nHost: test\r\nContent-Length: 2\r\n\r\nhi"]),
            ask(&["POST /echo HTTP/1.1\r\nHost: test\r\nContent-Le", "ngth: 5\r\n\r\nhe", "llo"]),
            ask(&["POST /echo HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"]),
            // The length the answer declares is the library's, worked out from the body.
            ask(&["POST /echo HTTP/1.1\r\ncontent-length: 12\r\n\r\nhello, world"]),
        ]
    } else {
        vec![]
    };
    drop(server.kill());
    drop(server.wait());
    assert!(up, "the service did not start listening");
    assert_eq!(answers[0], "HTTP/1.1 200\r\nContent-Length: 20\r\n\r\nhello from a package");
    assert_eq!(answers[1], answers[0], "a split request gets the same answer");
    assert_eq!(answers[2], "HTTP/1.1 404\r\nContent-Length: 0\r\n\r\n");
    assert_eq!(answers[3], "HTTP/1.1 200\r\nContent-Length: 2\r\n\r\nhi");
    assert_eq!(answers[4], "HTTP/1.1 200\r\nContent-Length: 5\r\n\r\nhello");
    assert_eq!(answers[5], "HTTP/1.1 501\r\nContent-Length: 0\r\n\r\n");
    assert_eq!(answers[6], "HTTP/1.1 200\r\nContent-Length: 12\r\n\r\nhello, world");
}

/// `ghttp`'s own suite, a client package of it: every case of `cases.txt` is parsed whole
/// and cut into two reads at every byte, and every cut must give what the whole gives.
#[test]
fn ghttp_parses_every_case_whole_and_split() {
    let world = World::new("ghttp_cases", &[]);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&world.dir)
        .env("GARDEL_CACHE_DIR", world.dir.join("cache"))
        .args(["--project", &root.join("ghttp/tests").to_string_lossy(), "--strict"])
        .output()
        .expect("gardelc runs");
    assert_ok(&out);
    let cases = std::fs::read(root.join("ghttp/tests/cases.txt")).expect("cases.txt");
    let mut run = Command::new(world.dir.join("build/ghttp_test_bin.exe"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the suite starts");
    run.stdin.take().expect("stdin").write_all(&cases).expect("cases are sent");
    let done = run.wait_with_output().expect("the suite ends");
    assert!(done.status.success(), "exit {:?}, failing case: {}", done.status.code(), String::from_utf8_lossy(&done.stdout));
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

// ---- Task E.5: qualified names ----------------------------------------------------------

fn run_binary(world: &World, package: &str, exe: &str) -> String {
    let run = Command::new(world.build_dir(package).join(exe)).output().expect("runs");
    String::from_utf8_lossy(&run.stdout).replace('\r', "")
}

const TWO_PARSERS: [(&str, &str); 2] = [("one", "{ path = \"../one\" }"), ("two", "{ path = \"../two\", as = \"OTHER\" }")];

fn two_parsers(world: &World) {
    world.package("one", &[], &[("parser.gdl", &parser(1))]);
    world.package("two", &[], &[("parser.gdl", &parser(2))]);
}

/// The Roadmap's acceptance: bare is refused, naming the spellings, and each qualified use
/// verifies and runs against its own package.
#[test]
fn a_qualifier_chooses_between_two_dependencies() {
    let world = World::new("qualified", &[]);
    two_parsers(&world);
    let bare = "functions CLIENT\n\nfeature {ANY}\n\n  use(p: PARSER): U64\n    do\n      p.size\n    end\n\nend\n";
    world.package("bare", TWO_PARSERS.as_slice(), &[("client.gdl", bare)]);
    let out = world.run("bare", &[]);
    assert_refused(&out, "E0105: ambiguous_module_reference");
    assert!(all(&out).contains("`ONE::PARSER`") && all(&out).contains("`OTHER::PARSER`"), "{}", all(&out));

    let app = "program QUALIFIED

feature {ANY}

  first_size(p: ONE::PARSER): U64
    do
      p.size
    ensure
      own_proof: Result >= 1
    end

  second_size(p: OTHER::PARSER): U64
    do
      p.size
    ensure
      own_proof: Result >= 2
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(if first_size(ONE::PARSER.Leaf(7)) = 1 then \"one\" else \"wrong\" end)
        IO.print_line(if second_size(OTHER::PARSER.Leaf(7)) = 2 then \"two\" else \"wrong\" end)
        IO.exit(0)
      end
    end

end
";
    world.package("root", TWO_PARSERS.as_slice(), &[("qualified.gdl", app)]);
    assert_ok(&world.run("root", &["--strict"]));
    assert_eq!(run_binary(&world, "root", "qualified_bin.exe"), "one\ntwo\n");
}

/// Qualification reaches type arguments at any depth, and a concept.
#[test]
fn a_qualified_name_works_inside_type_arguments() {
    let world = World::new("nested", &[]);
    two_parsers(&world);
    let client = "functions CLIENT

feature {ANY}

  first(xs: LIST[OPTION[ONE::PARSER]]): U64
    do
      0
    end

  second(pair: TUPLE[ONE::PARSER, LIST[OTHER::PARSER]]): U64
    do
      0
    end

end
";
    world.package("root", TWO_PARSERS.as_slice(), &[("client.gdl", client)]);
    assert_ok(&world.run("root", &["--strict"]));
}

/// A qualifier is allowed where nothing needs it, and it means what the bare name means.
#[test]
fn a_qualifier_that_is_not_needed_changes_nothing() {
    let client = |ty: &str, value: &str| {
        format!(
            "functions CLIENT\n\nfeature {{ANY}}\n\n  make: {ty}\n    do\n      {value}\n    end\n\nend\n"
        )
    };
    let world = World::new("needless", &[]);
    world.package("one", &[], &[("parser.gdl", &parser(1))]);
    world.package("bare", &[("one", "{ path = \"../one\" }")], &[("client.gdl", &client("PARSER", "Leaf(1)"))]);
    world.package("root", &[("one", "{ path = \"../one\" }")], &[("client.gdl", &client("ONE::PARSER", "ONE::PARSER.Leaf(1)"))]);
    assert_ok(&world.run("bare", &["--strict"]));
    assert_ok(&world.run("root", &["--strict"]));
    assert_eq!(world.build_file("bare", "client.bpl"), world.build_file("root", "client.bpl"));
}

/// An alias stays a spelling when the source uses it.
#[test]
fn renaming_a_used_alias_changes_no_symbol() {
    let world = World::new("alias_used", &[]);
    world.package("one", &[], &[("parser.gdl", &parser(1))]);
    let client = |alias: &str| {
        format!("functions CLIENT\n\nfeature {{ANY}}\n\n  make: {alias}::PARSER\n    do\n      {alias}::PARSER.Leaf(1)\n    end\n\nend\n")
    };
    world.package("a", &[("one", "{ path = \"../one\", as = \"FIRST\" }")], &[("client.gdl", &client("FIRST"))]);
    world.package("b", &[("one", "{ path = \"../one\", as = \"SECOND\" }")], &[("client.gdl", &client("SECOND"))]);
    assert_ok(&world.run("a", &["--strict"]));
    assert_ok(&world.run("b", &["--strict"]));
    assert_eq!(world.build_file("a", "client.bpl"), world.build_file("b", "client.bpl"));
    assert_eq!(world.build_file("a", "client.c"), world.build_file("b", "client.c"));
}

#[test]
fn a_qualified_name_has_to_exist() {
    let world = World::new("bad_qualifier", &[]);
    world.package("deep", &[], &[("deep_mod.gdl", "functions DEEP_MOD\n\nfeature {ANY}\n\n  v: U64\n    do\n      1\n    end\n\nend\n")]);
    world.package("one", &[("deep", "{ path = \"../deep\" }")], &[("parser.gdl", &parser(1))]);
    let root = |ty: &str| format!("functions CLIENT\n\nfeature {{ANY}}\n\n  use(p: {ty}): U64\n    do\n      0\n    end\n\nend\n");
    for (ty, expected) in [
        ("NOPE::PARSER", "`NOPE` is not an alias"),
        ("ONE::MISSING", "`ONE` provides no `MISSING`"),
        ("DEEP::DEEP_MOD", "`DEEP` is not an alias"),
    ] {
        world.package("root", &[("one", "{ path = \"../one\" }")], &[("client.gdl", &root(ty))]);
        let out = world.run("root", &[]);
        assert_refused(&out, "E0106: unknown_module");
        assert!(all(&out).contains(expected), "{}", all(&out));
    }
    let value = "functions CLIENT\n\nfeature {ANY}\n\n  make: ONE::PARSER\n    do\n      ONE::PARSER.Nothing(1)\n    end\n\nend\n";
    world.package("root", &[("one", "{ path = \"../one\" }")], &[("client.gdl", value)]);
    assert_refused(&world.run("root", &[]), "E0109: unknown_constructor");
}

/// The Roadmap's acceptance, with and without a manifest.
#[test]
fn a_user_command_and_the_preludes_work_in_one_module() {
    let app = "program USES_BOTH

feature {ANY}

  mine: COMMAND
    do
      Stop
    end

  weight(c: COMMAND): U64
    do
      match c
        case Stop then 5
        case Go(n) then n
      end
    end

  theirs: GARDEL::COMMAND
    do
      GARDEL::COMMAND.ReadLine
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(if weight(mine) = 5 then \"mine\" else \"wrong\" end)
        IO.exit(0)
      end
    end

end
";
    let command = "module COMMAND

representation {ANY}
  | Stop
  | Go(n: U64)

end
";
    let world = World::new("both_commands", &[]);
    world.package("root", &[], &[("command.gdl", command), ("uses_both.gdl", app)]);
    assert_ok(&world.run("root", &["--strict"]));
    assert_eq!(run_binary(&world, "root", "uses_both_bin.exe"), "mine\n");
}

/// With a user `EVENT`, `STEP` and `STRING`, a reactive program names the prelude's, and the
/// names `gardelc` builds in stay the prelude's: a string literal and the runtime's events.
#[test]
fn a_user_module_does_not_disturb_what_gardelc_builds_in() {
    let event = "module EVENT

representation {ANY}
  | Mine

end
";
    let step = "module STEP

representation {ANY}
  | Other

end
";
    let string = "module STRING

representation {ANY}
  | Text

end
";
    let app = "reactive program REACTS

feature {ANY}

  init(args: SEQ[GARDEL::STRING]): GARDEL::STEP[NAT]
    do
      Stop(0, LIST.empty.prepend(Print(\"hello\")))
    end

  step(s: NAT, e: GARDEL::EVENT): GARDEL::STEP[NAT]
    do
      Stop(0, LIST.empty)
    end

end
";
    let world = World::new("built_in", &[]);
    world.package("root", &[], &[("event.gdl", event), ("step.gdl", step), ("string.gdl", string), ("reacts.gdl", app)]);
    assert_ok(&world.run("root", &["--strict"]));
    assert_eq!(run_binary(&world, "root", "reacts_bin.exe"), "hello
");
}

const WITH_ERROR: &str = "module OUTCOME

representation {ANY}
  | Fine
  | Error(code: U64)

end
";

/// The Roadmap's acceptance: a type known for certain breaks a tie, and nothing else does.
#[test]
fn a_known_type_breaks_a_constructor_tie() {
    let world = World::new("tie", &[]);
    world.package("dep", &[], &[("outcome.gdl", WITH_ERROR)]);
    let dep = [("dep", "{ path = \"../dep\" }")];
    let known = "program TIE

feature {ANY}

  of_param(r: RESULT[U64, U64]): U64
    do
      match r
        case Ok(v) then v
        case Error(e) then e
      end
    end

  of_routine: U64
    do
      match failed
        case Ok(v) then v
        case Error(e) then e
      end
    end

  failed: RESULT[U64, U64]
    do
      Error(4)
    end

  of_let: U64
    do
      let r: RESULT[U64, U64] = failed
      match r
        case Ok(v) then v
        case Error(e) then e
      end
    end

  of_outcome(o: OUTCOME): U64
    do
      match o
        case Fine then 0
        case Error(c) then c
      end
    end

  of_plain_let: U64
    do
      let r = failed
      match r
        case Ok(v) then v
        case Error(e) then e
      end
    end

  through_if(b: BOOL): RESULT[U64, U64]
    do
      if b then
        Error(1)
      else
        Ok(3)
      end
    end

  through_match(o: OPTION[U64]): RESULT[U64, U64]
    do
      match o
        case Some(v) then Error(v)
        case None then Ok(3)
      end
    end

  nested(o: OPTION[RESULT[U64, U64]]): U64
    do
      match o
        case Some(Ok(v)) then v
        case Some(GARDEL::RESULT.Error(e)) then e
        case None then 0
      end
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        line <- IO.read_line
        match line
          case Ok(text) then
            with IO
              IO.print_line(if of_outcome(OUTCOME.Error(6)) + of_routine = 10 then \"chosen\" else \"wrong\" end)
              IO.exit(0)
            end
          case Error(e) then
            with IO
              IO.exit(1)
            end
        end
      end
    end

end
";
    world.package("root", &dep, &[("tie.gdl", known)]);
    assert_ok(&world.run("root", &["--strict"]));

    let unknown = "functions NO_TYPE\n\nfeature {ANY}\n\n  v: U64\n    do\n      let x = Error(1)\n      0\n    end\n\nend\n";
    world.package("root", &dep, &[("no_type.gdl", unknown)]);
    let out = world.run("root", &[]);
    assert_refused(&out, "E0112: ambiguous_constructor");
    assert!(all(&out).contains("`GARDEL::RESULT.Error`") && all(&out).contains("`DEP::OUTCOME.Error`"), "{}", all(&out));

    // The type of a `match` is its scrutinee's, and says nothing of a nested pattern's.
    let nested = "functions NESTED

feature {ANY}

  v(o: OPTION[RESULT[U64, U64]]): U64
    do
      match o
        case Some(Error(e)) then e
        case _ then 0
      end
    end

end
";
    world.package("root", &dep, &[("nested.gdl", nested)]);
    assert_refused(&world.run("root", &[]), "E0112: ambiguous_constructor");
}

/// A tie-break only breaks a tie: a module's own constructor wins even against the type, so
/// such a module writes the qualified spelling.
#[test]
fn a_known_type_does_not_override_a_nearer_constructor() {
    let job = "module JOB

representation {ANY}
  | ReadKey(reason: NAT)
  | Idle

feature {ANY}

  to_command: COMMAND
    do
      ReadKey(1)
    end

end
";
    let world = World::new("nearest_ctor", &[]);
    world.package("root", &[], &[("job.gdl", job)]);
    assert!(!world.run("root", &[]).status.success());
    world.package("fixed", &[], &[("job.gdl", &job.replace("      ReadKey(1)", "      COMMAND.ReadKey"))]);
    assert_ok(&world.run("fixed", &["--strict"]));
}

/// Decision 9's first examples: two modules of one package with one constructor.
#[test]
fn a_constructor_is_qualified_by_its_module() {
    let tok_a = "module TOK_A\n\nrepresentation {ANY}\n  | Leaf(a: U64)\n\nfeature {ANY}\n\n  value: U64\n    do\n      match Current\n        case Leaf(x) then x\n      end\n    end\n\n  untyped: U64\n    do\n      let own = Leaf(1)\n      0\n    end\n\nend\n";
    let tok_b = "module TOK_B\n\nrepresentation {ANY}\n  | Leaf(b: U64)\n\nfeature {ANY}\n\n  value: U64\n    do\n      match Current\n        case Leaf(x) then x\n      end\n    end\n\nend\n";
    let app = "program PICKS

feature {ANY}

  a_of(v: TOK_A): U64
    do
      match v
        case TOK_A.Leaf(x) then x
      end
    end

  b_of(v: TOK_B): U64
    do
      match v
        case Leaf(x) then x
      end
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(if a_of(TOK_A.Leaf(1)) = 1 then \"one\" else \"wrong\" end)
        IO.print_line(if b_of(TOK_B.Leaf(2)) = 2 then \"two\" else \"wrong\" end)
        IO.print_line(if TOK_B.Leaf(3).value = 3 then \"three\" else \"wrong\" end)
        IO.exit(0)
      end
    end

end
";
    let world = World::new("by_module", &[]);
    world.package("root", &[], &[("tok_a.gdl", tok_a), ("tok_b.gdl", tok_b), ("picks.gdl", app)]);
    assert_ok(&world.run("root", &["--strict"]));
    assert_eq!(run_binary(&world, "root", "picks_bin.exe"), "one\ntwo\nthree\n");

    // A failure speaks of the source: its span finds `Leaf`, and its text has no internal name.
    let wrong = tok_b.replace("then x\n", "then x + 1\n");
    world.package("root", &[], &[("tok_a.gdl", tok_a), ("tok_b.gdl", &wrong), ("picks.gdl", app)]);
    let failed = world.run("root", &[]);
    assert!(!failed.status.success());
    assert!(!all(&failed).contains("PK5_LOCAL"), "{}", all(&failed));

    let bare = "functions BARE\n\nfeature {ANY}\n\n  v: U64\n    do\n      let x = Leaf(1)\n      0\n    end\n\nend\n";
    world.package("root", &[], &[("tok_a.gdl", tok_a), ("tok_b.gdl", tok_b), ("bare.gdl", bare)]);
    let out = world.run("root", &[]);
    assert_refused(&out, "E0112: ambiguous_constructor");
    assert!(all(&out).contains("`TOK_A.Leaf`") && all(&out).contains("`TOK_B.Leaf`"), "{}", all(&out));
}

/// A qualifier is a spelling. It opens nothing that the bare name would not.
#[test]
fn a_qualifier_opens_no_boundary() {
    let world = World::new("qualified_hidden", &[]);
    let secret = "module SECRET

representation {NONE}
  | Sealed(v: U64)

create {ANY}

  make: SECRET
    do
      Sealed(1)
    end

feature {NONE}

  hidden: U64
    do
      1
    end

end
";
    let tool = "program TOOL\n\nfeature {ANY}\n\n  main(args: SEQ[STRING]): IO[EXIT_CODE]\n    do\n      with IO\n        IO.exit(0)\n      end\n    end\n\nend\n";
    world.package("dep", &[], &[("secret.gdl", secret), ("tool.gdl", tool)]);
    let dep = [("dep", "{ path = \"../dep\" }")];
    let client = |body: &str| format!("functions CLIENT\n\nfeature {{ANY}}\n\n  v(s: DEP::SECRET): U64\n    do\n      {body}\n    end\n\nend\n");
    for (body, rule) in [
        ("let x = DEP::SECRET.Sealed(1)\n      0", "E0113: hidden_member"),
        ("match s\n        case DEP::SECRET.Sealed(x) then x\n      end", "E0113: hidden_member"),
        ("s.hidden", "E0113: hidden_member"),
        ("let x = DEP::TOOL.main(LIST.empty)\n      0", "E0104"),
    ] {
        world.package("root", &dep, &[("client.gdl", &client(body))]);
        assert_refused(&world.run("root", &[]), rule);
    }
}

/// A package with one constructor in two modules, seen from a client.
#[test]
fn a_clients_qualifier_reaches_a_constructor_a_package_declares_twice() {
    let world = World::new("twice", &[]);
    let tok_a = "module TOK_A\n\nrepresentation {ANY}\n  | Leaf(a: U64)\n\nend\n";
    let tok_b = "module TOK_B\n\nrepresentation {ANY}\n  | Leaf(b: U64)\n\nend\n";
    world.package("toks", &[], &[("tok_a.gdl", tok_a), ("tok_b.gdl", tok_b)]);
    let dep = [("toks", "{ path = \"../toks\" }")];
    let good = "functions CLIENT

feature {ANY}

  of_b(t: TOKS::TOK_B): U64
    do
      match t
        case TOKS::TOK_B.Leaf(x) then x
      end
    end

  known(t: TOK_A): U64
    do
      match t
        case Leaf(x) then x
      end
    end

  made: U64
    do
      of_b(TOKS::TOK_B.Leaf(3))
    ensure
      three: Result = 3
    end

end
";
    world.package("root", &dep, &[("client.gdl", good)]);
    assert_ok(&world.run("root", &["--strict"]));

    // Two owners in a package are ambiguous with no type, and across `::` as well.
    for body in ["let x = Leaf(1)\n      0", "let x = TOKS::Leaf(1)\n      0"] {
        let bad = format!("functions CLIENT\n\nfeature {{ANY}}\n\n  v: U64\n    do\n      {body}\n    end\n\nend\n");
        world.package("root", &dep, &[("client.gdl", &bad)]);
        let out = world.run("root", &[]);
        assert_refused(&out, "E0112: ambiguous_constructor");
        assert!(all(&out).contains("TOK_B.Leaf"), "{}", all(&out));
    }
}

/// `EQ` is a built-in concept by name, and a user's is another one.
#[test]
fn a_user_concept_of_a_prelude_name_is_its_own() {
    let mine = "concept EQ\n  same(other: Current): BOOL\nend\n";
    let thing = "module THING satisfies EQ

representation {ANY}
  | Only

feature {ANY}

  same(other: THING): BOOL
    do
      True
    end

end
";
    let world = World::new("user_eq", &[]);
    world.package("root", &[], &[("eq.gdl", mine), ("thing.gdl", thing)]);
    assert_ok(&world.run("root", &["--strict"]));
}

/// With no manifest there is still a package, so all of this works on plain files.
#[test]
fn a_project_with_no_manifest_can_reuse_a_prelude_name() {
    let world = World::new("no_manifest", &[]);
    world.write("plain/option.gdl", "module OPTION\n\nrepresentation {ANY}\n  | Some(mine: U64)\n  | Nothing\n\nend\n");
    world.write(
        "plain/client.gdl",
        "functions CLIENT

feature {ANY}

  mine(o: OPTION): U64
    do
      match o
        case Some(v) then v
        case Nothing then 0
      end
    end

  theirs(o: GARDEL::OPTION[U64]): U64
    do
      match o
        case GARDEL::OPTION.Some(v) then v
        case None then 1
      end
    end

end
",
    );
    assert_ok(&world.run("plain", &["client.gdl", "option.gdl", "--strict"]));

    // The project's name reads as it is written, with no package in front of it.
    world.write("plain/client.gdl", "functions CLIENT\n\nfeature {ANY}\n\n  mine(o: OPTION): U64\n    do\n      o.nothing\n    end\n\nend\n");
    let out = world.run("plain", &["client.gdl", "option.gdl"]);
    assert!(!out.status.success());
    assert!(all(&out).contains("OPTION") && !all(&out).contains("local::"), "{}", all(&out));
}

/// `M.Leaf` names `M`'s, and another module's `Leaf` is not a spelling of it.
#[test]
fn a_constructor_belongs_to_the_module_named() {
    let world = World::new("wrong_owner", &[]);
    let has_it = "module HAS_IT\n\nrepresentation {ANY}\n  | Mine(v: U64)\n\nend\n";
    let lacks_it = "module LACKS_IT\n\nrepresentation {ANY}\n  | Other(v: U64)\n\nend\n";
    let client = "functions USER\n\nfeature {ANY}\n\n  v: LACKS_IT\n    do\n      LACKS_IT.Mine(1)\n    end\n\nend\n";
    world.package("root", &[], &[("has_it.gdl", has_it), ("lacks_it.gdl", lacks_it), ("user.gdl", client)]);
    assert_refused(&world.run("root", &[]), "E0109: unknown_constructor");

    let pattern = "functions USER\n\nfeature {ANY}\n\n  v(l: LACKS_IT): U64\n    do\n      match l\n        case LACKS_IT.Mine(x) then x\n        case Other(x) then x\n      end\n    end\n\nend\n";
    world.package("root", &[], &[("has_it.gdl", has_it), ("lacks_it.gdl", lacks_it), ("user.gdl", pattern)]);
    assert_refused(&world.run("root", &[]), "E0109: unknown_constructor");
}

/// A constructor two modules declare reads as `MODULE.Leaf`, and is found in the source.
#[test]
fn a_diagnostic_names_a_constructor_by_its_module() {
    let world = World::new("readable_ctor", &[]);
    let tok_a = "module TOK_A\n\nrepresentation {NONE}\n  | Leaf(a: U64)\n\nend\n";
    let tok_b = "module TOK_B\n\nrepresentation {NONE}\n  | Leaf(b: U64)\n\nend\n";
    let client = "functions USER\n\nfeature {ANY}\n\n  v: U64\n    do\n      let x = TOK_B.Leaf(1)\n      0\n    end\n\nend\n";
    world.package("root", &[], &[("tok_a.gdl", tok_a), ("tok_b.gdl", tok_b), ("user.gdl", client)]);
    let out = world.run("root", &[]);
    assert_refused(&out, "E0113: hidden_member");
    let text = all(&out);
    assert!(text.contains("`root::TOK_B.Leaf`"), "{text}");
    assert!(text.contains("user.gdl:7:"), "{text}");
}
