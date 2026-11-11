//! A proof can read bytes at an offset (F.8): a string literal's lemma proves its bytes, an
//! item of a concatenation is one of its parts, and an item of a slice is the item of the
//! whole at its offset. The bytes are a lemma of their own, given only to a routine that reads
//! the bytes of a string.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const PROGRAM: &str = "functions WIRE

feature {ANY}

  -- \"GET\" holds no space, and \"HTTP\" is ASCII.
  method_ok: BOOL
    do
      True
    ensure
      no_space: forall k: NAT. k < \"GET\".byte_length implies LIST_OPS.at(\"GET\".byte_model, k) != 32
      first: LIST_OPS.at(\"HTTP\".to_utf8.model, 0) = 72
    end

  -- The third byte of `a` then `b` is the first of `b`, when `a` has two.
  joined(a: STRING, b: STRING): BOOL
    require
      two: a.byte_length = 2
      some: b.byte_length >= 1
      fits: a.byte_length <= 1000 and b.byte_length <= 1000
    do
      True
    ensure
      third: LIST_OPS.at(a.concat(b).byte_model, 2) = LIST_OPS.at(b.byte_model, 0)
    end

  -- An item of a slice is the item of the whole at the slice's offset.
  sliced(bs: BYTES, k: NAT): BOOL
    require
      inside: k + 2 < bs.length
    do
      True
    ensure
      shifted: LIST_OPS.at(bs.slice(2, bs.length).model, k) = LIST_OPS.at(bs.model, k + 2)
CLAIM    end

end
";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn check(name: &str, claim: &str) -> (Dir, Output) {
    let dir = Dir(std::env::temp_dir().join(format!("gardelc_literal_bytes_{}_{name}", std::process::id())));
    drop(fs::remove_dir_all(&dir.0));
    fs::create_dir_all(&dir.0).expect("temporary directory");
    fs::write(dir.0.join("wire.gdl"), PROGRAM.replace("CLAIM", claim)).expect("source");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir.0)
        .env("GARDEL_CACHE_DIR", dir.0.join("cache"))
        .args(["wire.gdl", "--strict"])
        .output()
        .expect("gardelc runs");
    (dir, out)
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn a_proof_reads_bytes_at_an_offset() {
    let (_dir, out) = check("ok", "");
    let text = all(&out);
    assert!(out.status.success() && text.contains("0 errors, 0 warnings"), "{text}");
}

/// The facts are the bytes there are, no more: a wrong offset is refused.
#[test]
fn a_claim_past_the_bytes_is_refused() {
    let (_dir, out) = check("lie", "      lie: LIST_OPS.at(bs.slice(2, bs.length).model, k) = LIST_OPS.at(bs.model, k + 1)\n");
    assert!(!out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("condition `lie` failed"), "{}", all(&out));
}
