//! A module on which the solver gives up under Z3's default seed is verified again under
//! seeds 1, 2 and 3 (H.0c), unless the run fixed a seed. A run that still gives up is
//! reported as unproven, as before.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, source: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_seed_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        fs::write(dir.join("Gardel.toml"), format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", name.replace('_', "-")))
            .expect("manifest");
        fs::write(dir.join("src").join("work.gdl"), source).expect("source file");
        Self { dir }
    }

    fn check(&self, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&self.dir)
            .env("GARDEL_CACHE_DIR", self.dir.join("cache"))
            .env_remove("GARDEL_Z3_SEED")
            .args(["--project", ".", "--strict"])
            .args(extra)
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

const WORK: &str = "module WORK

create {ANY}

  sum(a: NAT, b: NAT, c: NAT): BOOL
    require
      small: a < 1000 and b < 1000 and c < 1000
    do
      (a + b) * c = a * c + b * c
    ensure
      holds: Result
    end

end
";

/// With a budget of one unit, Z3 gives up under every seed: the module is tried under the
/// default seed and seeds 1 to 3, and reported as unproven. With `--z3-seed`, it is tried
/// under that seed alone. With the default budget it is proven at once, and not retried.
#[test]
fn a_module_the_solver_gives_up_on_is_tried_under_other_seeds() {
    let p = Project::new("work", WORK);
    let starved = p.check(&["--z3-rlimit", "1"]);
    let text = all(&starved);
    assert!(!starved.status.success(), "{text}");
    assert!(text.contains("Verification Resource Limit") || text.contains("Verification Timeout"), "{text}");
    assert!(text.contains("[seed] `WORK`: the solver gave up under Z3's default seed and under seeds 1, 2 and 3"), "{text}");

    let fixed = p.check(&["--z3-rlimit", "1", "--z3-seed", "7"]);
    let text = all(&fixed);
    assert!(!fixed.status.success(), "{text}");
    assert!(!text.contains("[seed]"), "a fixed seed is not varied:\n{text}");

    let proven = p.check(&[]);
    let text = all(&proven);
    assert!(proven.status.success() && text.contains("0 errors, 0 warnings"), "{text}");
    assert!(!text.contains("[seed]"), "{text}");
}
