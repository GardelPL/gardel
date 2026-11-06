//! A directory with a `Gardel.toml` is a project however it is named on the command line.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MANIFEST: &str = "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n";

const FINE: &str = "functions FINE

feature {ANY}

  f(x: U64): U64
    do
      1
    ensure
      one: Result = 1
    end

end
";

/// Outside `src/`, so not part of a manifest project. False, so linking it fails the run.
const STRAY: &str = "functions STRAY

feature {ANY}

  f(x: U64): U64
    do
      1
    ensure
      two: Result = 2
    end

end
";

/// A directory of its own for `name`, with `fine.gdl` in `src/` and `stray.gdl` in `old/`.
fn project(name: &str, manifest: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gardelc_operand_{}_{name}", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(dir.join("demo/src")).expect("source directory");
    fs::create_dir_all(dir.join("demo/old")).expect("other directory");
    fs::write(dir.join("demo/src/fine.gdl"), FINE).expect("source file");
    fs::write(dir.join("demo/old/stray.gdl"), STRAY).expect("source file");
    if let Some(manifest) = manifest {
        fs::write(dir.join("demo/Gardel.toml"), manifest).expect("manifest");
    }
    dir
}

fn gardelc(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(dir)
        .env("GARDEL_CACHE_DIR", dir.join("cache"))
        .args(args)
        .output()
        .expect("gardelc runs")
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn a_directory_with_a_manifest_is_that_project() {
    let dir = project("same", Some(MANIFEST));
    for args in [&["demo"][..], &["./demo/"][..], &["--project", "demo"][..]] {
        let out = gardelc(&dir, args);
        assert!(out.status.success(), "{args:?}: {}", text(&out));
        assert!(!text(&out).contains("STRAY"), "{args:?}: {}", text(&out));
    }
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn the_manifest_of_a_directory_is_read() {
    let dir = project("bad_manifest", Some("[project]\nname = \"Not Valid\"\nversion = \"0.1.0\"\n"));
    let out = gardelc(&dir, &["demo"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("Gardel.toml"), "{}", text(&out));
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn a_project_does_not_mix_with_other_inputs() {
    let dir = project("mixed", Some(MANIFEST));
    let out = gardelc(&dir, &["demo", "demo/old/stray.gdl"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("Gardel.toml"), "{}", text(&out));
    assert!(!text(&out).contains("Postcondition"), "{}", text(&out));
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn a_directory_without_a_manifest_stands_for_all_its_files() {
    let dir = project("plain", None);
    let out = gardelc(&dir, &["demo"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("Postcondition"), "{}", text(&out));
    drop(fs::remove_dir_all(&dir));
}
