//! A call of another module's routine that has the caller's name is no recursive call (F.11d).
//!
//! In `B_FNS.named`, the call `A_FNS.named(..)` was taken for a call of `B_FNS.named` itself,
//! so its arguments had to make the variant decrease: `variant_decrease` failed, and the
//! routine's contract was withheld from its own proof. So was `x.size` on a `WRAP` inside a
//! routine `size` of another module. A real recursive call, `B_FNS.named(names.tail, ..)`,
//! still has to decrease it: `C_FNS.named`, which does not, is refused.

use std::fs;
use std::process::Command;

const A_FNS: &str = "functions A_FNS

feature {ANY}

  named(xs: LIST[NAT], n: NAT): BOOL
    variant
      xs.length
    do
      if xs.length = 0 then False else xs.head = n or A_FNS.named(xs.tail, n) end
    end

end
";

const B_FNS: &str = "functions B_FNS

feature {ANY}

  -- How many of `names` are in `ms`, at most 1000.
  named(names: LIST[NAT], ms: LIST[NAT]): NAT
    variant
      names.length
    do
      if names.length = 0 then
        0
      else
        let hit = A_FNS.named(ms, names.head)
        let rest = B_FNS.named(names.tail, ms)
        if hit and rest < 1000 then rest + 1 else rest end
      end
    ensure
      at_most: Result <= 1000
    end

  -- The sizes of `ws`, added up to at most 1000.
  size(ws: LIST[WRAP]): NAT
    variant
      ws.length
    do
      if ws.length = 0 then
        0
      else
        let rest = B_FNS.size(ws.tail)
        let n = ws.head.size
        if rest < 500 and n < 500 then rest + n else rest end
      end
    ensure
      at_most: Result <= 1000
    end

end
";

const WRAP: &str = "module WRAP

representation {NONE}
  n: NAT

create {ANY}

  make(k: NAT): WRAP
    do
      { n = k }
    end

feature {ANY}

  size: NAT
    do
      Current.n
    end

end
";

const C_FNS: &str = "functions C_FNS

feature {ANY}

  named(names: LIST[NAT], ms: LIST[NAT]): NAT
    variant
      names.length
    do
      if names.length = 0 then 0 else C_FNS.named(names, ms.tail) end
    end

end
";

fn check(name: &str, files: &[(&str, &str)]) -> String {
    let dir = std::env::temp_dir().join(format!("gardelc_variant_scope_{}_{name}", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(dir.join("src")).expect("temporary directory");
    fs::write(dir.join("Gardel.toml"), "[project]\nname = \"scope\"\nversion = \"0.1.0\"\n").expect("manifest");
    for (file, text) in files {
        fs::write(dir.join("src").join(file), text).expect("source");
    }
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir)
        .env("GARDEL_CACHE_DIR", dir.join("cache"))
        .args(["--project", ".", "--strict"])
        .output()
        .expect("gardelc runs");
    drop(fs::remove_dir_all(&dir));
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn a_call_of_another_modules_routine_of_the_same_name_is_no_recursion() {
    let text = check("other", &[("a_fns.gdl", A_FNS), ("b_fns.gdl", B_FNS), ("wrap.gdl", WRAP)]);
    assert!(text.contains("0 errors, 0 warnings"), "{text}");
}

#[test]
fn a_recursive_call_that_does_not_decrease_the_variant_is_still_refused() {
    let text = check("own", &[("c_fns.gdl", C_FNS)]);
    assert!(text.contains("in `named` (variant_decrease)"), "{text}");
}
