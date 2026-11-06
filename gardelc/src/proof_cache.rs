//! The local verification cache (Task E.3, decision 1). It saves running the solver again on
//! a verification condition this machine has already proven, and it is not evidence.
//!
//! The key hashes the exact text given to Boogie, with everything else that decides Boogie's
//! answer. So a hit is the answer a run would give, and a change a client's verification
//! condition cannot see, such as a dependency's body inside its contract, keeps its entry.
//! Only a clean result is stored.
//!
//! An entry counts only if it is byte for byte the one this machine would write, and that
//! includes a code keyed by a secret that stays in the cache directory. A mutated, moved or
//! copied entry, or one written elsewhere, is a miss, and a miss runs the solver. The cache
//! lives outside the project, so nothing that arrives with a package is ever read. Whoever can
//! read this user's secret can forge an entry, but could equally replace `gardelc`.

use std::cell::{Cell, OnceCell};
use std::env;
use std::fs;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::path::{Path, PathBuf};

/// Part of every key, so a change to the format or key below re-verifies everything.
const FORMAT: &str = "gardel proof cache 1";
const SECRET_FILE: &str = "machine.secret";

/// What decides a solver's answer, other than the verifier and the tools.
#[derive(Debug, Clone, Copy)]
pub struct Query<'a> {
    pub bpl: &'a str,
    pub rlimit: u64,
    pub seed: Option<u32>,
    pub fuel: usize,
}

/// How much of a run the cache answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    pub from_cache: usize,
    pub verified: usize,
}

pub struct ProofCache {
    dir: PathBuf,
    secret: [u8; 32],
    verifier: String,
    /// `--verify-deps`: every proof is run again. Its results are still stored.
    reads: bool,
    /// Boogie's and Z3's versions, once asked for. `None` if they could not be found out.
    tools: OnceCell<Option<String>>,
    from_cache: Cell<usize>,
    verified: Cell<usize>,
}

impl ProofCache {
    /// Opens the cache in `GARDEL_CACHE_DIR` or the user's cache directory.
    pub fn open(verifier: String, reads: bool) -> Result<Self, String> {
        let dir = default_dir().ok_or("no cache directory: set `GARDEL_CACHE_DIR`")?;
        Self::open_at(&dir, verifier, reads)
    }

    pub fn open_at(dir: &Path, verifier: String, reads: bool) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|e| format!("cannot use `{}`: {e}", dir.display()))?;
        let secret = read_secret(dir).map_or_else(|| write_secret(dir), Ok)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            secret,
            verifier,
            reads,
            tools: OnceCell::new(),
            from_cache: Cell::new(0),
            verified: Cell::new(0),
        })
    }

    /// The versions of the tools, found once per run. Without them nothing can be looked up
    /// or stored, so a run with a broken solver cannot succeed from the cache.
    pub fn tools(&self, find: impl FnOnce() -> Option<String>) -> Option<&str> {
        self.tools.get_or_init(find).as_deref()
    }

    /// The key of `query`, in hex. Each input is length-prefixed, so no two sets of inputs
    /// give one text.
    pub fn key(&self, tools: &str, query: &Query<'_>) -> String {
        let mut text = Vec::new();
        let fields = [
            FORMAT,
            &self.verifier,
            tools,
            &query.rlimit.to_string(),
            &query.seed.map_or_else(|| "none".to_string(), |s| s.to_string()),
            &query.fuel.to_string(),
            query.bpl,
        ];
        for field in fields {
            text.extend_from_slice(format!("{}:", field.len()).as_bytes());
            text.extend_from_slice(field.as_bytes());
        }
        hex(&sha256(&text))
    }

    /// Whether this machine proved `key` for `module`, resting on exactly `assumes`.
    pub fn holds(&self, key: &str, module: &str, assumes: &[String]) -> bool {
        let hit = self.reads
            && fs::read(self.entry_path(key)).is_ok_and(|bytes| bytes == self.entry(key, module, assumes).as_bytes());
        if hit {
            self.from_cache.set(self.from_cache.get() + 1);
        }
        hit
    }

    /// Records a clean result. A cache that cannot be written only costs time.
    pub fn store(&self, key: &str, module: &str, assumes: &[String]) {
        write_atomically(&self.entry_path(key), self.entry(key, module, assumes).as_bytes());
    }

    /// Counts a module the solver had to run on.
    pub fn count_run(&self) {
        self.verified.set(self.verified.get() + 1);
    }

    pub fn tally(&self) -> Tally {
        Tally { from_cache: self.from_cache.get(), verified: self.verified.get() }
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.proof"))
    }

    /// The one text that counts as the entry of `key`.
    fn entry(&self, key: &str, module: &str, assumes: &[String]) -> String {
        let mut text = format!("{FORMAT}\nkey {key}\nmodule {module}\n");
        for assume in assumes {
            text.push_str(&format!("assume {assume}\n"));
        }
        let code = hex(&hmac_sha256(&self.secret, text.as_bytes()));
        text.push_str(&format!("code {code}\n"));
        text
    }
}

fn default_dir() -> Option<PathBuf> {
    let var = |name| env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    var("GARDEL_CACHE_DIR")
        .or_else(|| var("LOCALAPPDATA").map(|d| d.join("gardel").join("cache")))
        .or_else(|| var("XDG_CACHE_HOME").map(|d| d.join("gardel")))
        .or_else(|| var("HOME").map(|d| d.join(".cache").join("gardel")))
}

fn read_secret(dir: &Path) -> Option<[u8; 32]> {
    fs::read(dir.join(SECRET_FILE)).ok()?.try_into().ok()
}

/// Makes a secret. If two runs race, the last one's stands, and the other's entries are misses.
fn write_secret(dir: &Path) -> Result<[u8; 32], String> {
    let path = dir.join(SECRET_FILE);
    let secret = fresh_secret();
    if !write_atomically(&path, &secret) {
        return Err(format!("cannot write `{}`", path.display()));
    }
    read_secret(dir).ok_or_else(|| format!("cannot read `{}`", path.display()))
}

/// 32 bytes from the operating system's random source, which seeds `RandomState`. This keeps
/// `gardelc` free of dependencies. The result is as strong as those 128 bits of seed.
fn fresh_secret() -> [u8; 32] {
    let mut seed = Vec::new();
    for i in 0..4_u8 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u8(i);
        seed.extend_from_slice(&hasher.finish().to_le_bytes());
    }
    sha256(&seed)
}

/// Writes through a temporary file, so a reader never sees half a file. On Unix only this user
/// can read it, which matters for the secret.
fn write_atomically(path: &Path, bytes: &[u8]) -> bool {
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    let written = {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&temp).and_then(|mut f| std::io::Write::write_all(&mut f, bytes))
    };
    let moved = written.and_then(|()| fs::rename(&temp, path));
    if moved.is_err() {
        drop(fs::remove_file(&temp));
    }
    moved.is_ok()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

const K: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
    0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
    0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
    0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
    0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
    0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
    0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
    0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
    0xc671_78f2,
];

/// SHA-256 (FIPS 180-4).
pub(crate) fn sha256(message: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    let bits = u64::try_from(message.len()).expect("a message fits in 64 bits") * 8;
    padded.extend_from_slice(&bits.to_be_bytes());

    for block in padded.chunks_exact(64) {
        let mut w = [0_u32; 64];
        for (word, bytes) in w.iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes(bytes.try_into().expect("a chunk of four bytes"));
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for t in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[t]).wrapping_add(w[t]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(value);
        }
    }

    let mut digest = [0_u8; 32];
    for (bytes, word) in digest.chunks_exact_mut(4).zip(h) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// HMAC-SHA-256 (RFC 2104), for a key of at most one block.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    assert!(key.len() <= 64, "the secret is shorter than a block");
    let mut inner = vec![0x36_u8; 64];
    let mut outer = vec![0x5c_u8; 64];
    for (i, byte) in key.iter().enumerate() {
        inner[i] ^= byte;
        outer[i] ^= byte;
    }
    inner.extend_from_slice(message);
    outer.extend_from_slice(&sha256(&inner));
    sha256(&outer)
}

#[cfg(test)]
mod tests {
    use super::{Query, ProofCache, hex, hmac_sha256, sha256};
    use std::fs;
    use std::path::PathBuf;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gardelc_cache_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        dir
    }

    const QUERY: Query<'static> = Query { bpl: "procedure p();", rlimit: 1, seed: None, fuel: 2 };

    #[test]
    fn sha256_matches_the_fips_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Two blocks of padding, and a message longer than a block.
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            hex(&sha256(&vec![b'a'; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn hmac_matches_rfc_4231() {
        assert_eq!(
            hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn every_input_changes_the_key() {
        let at = dir("key");
        let cache = ProofCache::open_at(&at, "v1".into(), true).expect("cache");
        let base = cache.key("tools", &QUERY);
        assert_eq!(base, cache.key("tools", &QUERY));
        let changed = [
            cache.key("other tools", &QUERY),
            cache.key("tools", &Query { bpl: "procedure q();", ..QUERY }),
            cache.key("tools", &Query { rlimit: 2, ..QUERY }),
            cache.key("tools", &Query { seed: Some(1), ..QUERY }),
            cache.key("tools", &Query { fuel: 3, ..QUERY }),
            ProofCache::open_at(&at, "v2".into(), true).expect("cache").key("tools", &QUERY),
        ];
        assert!(changed.iter().all(|k| *k != base), "{changed:?}");
        // Length prefixes: moving a character between two inputs is a different key.
        let moved = ProofCache::open_at(&at, "v".into(), true).expect("cache");
        assert_ne!(moved.key("1tools", &QUERY), cache.key("tools", &QUERY));
        drop(fs::remove_dir_all(&at));
    }

    #[test]
    fn only_the_entry_this_machine_wrote_counts() {
        let at = dir("entry");
        let cache = ProofCache::open_at(&at, "v1".into(), true).expect("cache");
        let key = cache.key("tools", &QUERY);
        let assumes = vec!["f hole".to_string()];
        assert!(!cache.holds(&key, "M", &assumes), "nothing is stored yet");

        cache.store(&key, "M", &assumes);
        assert!(cache.holds(&key, "M", &assumes));
        assert!(!cache.holds(&key, "N", &assumes), "the entry of another module");
        assert!(!cache.holds(&key, "M", &[]), "an entry with other assumes");

        let path = at.join(format!("{key}.proof"));
        let good = fs::read(&path).expect("entry");

        // Any byte changed. The last is a newline, so this covers the whole entry.
        for i in 0..good.len() {
            let mut bad = good.clone();
            bad[i] ^= 1;
            fs::write(&path, &bad).expect("mutated entry");
            assert!(!cache.holds(&key, "M", &assumes), "byte {i}");
        }

        // A correct entry moved to another key's name.
        let other = cache.key("tools", &Query { fuel: 9, ..QUERY });
        fs::write(at.join(format!("{other}.proof")), &good).expect("moved entry");
        assert!(!cache.holds(&other, "M", &assumes));

        // Written with another machine's secret.
        let elsewhere_at = dir("entry_elsewhere");
        let elsewhere = ProofCache::open_at(&elsewhere_at, "v1".into(), true).expect("cache");
        assert_eq!(elsewhere.key("tools", &QUERY), key, "a key is not secret");
        elsewhere.store(&key, "M", &assumes);
        let forged = fs::read(elsewhere_at.join(format!("{key}.proof"))).expect("entry");
        assert_ne!(forged, good);
        fs::write(&path, &forged).expect("forged entry");
        assert!(!cache.holds(&key, "M", &assumes));

        // The genuine entry still counts, so the refusals above came from the changes.
        fs::write(&path, &good).expect("entry");
        assert!(cache.holds(&key, "M", &assumes));
        drop(fs::remove_dir_all(&at));
        drop(fs::remove_dir_all(&elsewhere_at));
    }

    #[test]
    fn the_secret_outlives_a_run_and_differs_between_caches() {
        let (a, b) = (dir("secret_a"), dir("secret_b"));
        let first = ProofCache::open_at(&a, "v1".into(), true).expect("cache");
        let key = first.key("tools", &QUERY);
        first.store(&key, "M", &[]);
        let again = ProofCache::open_at(&a, "v1".into(), true).expect("cache");
        assert!(again.holds(&key, "M", &[]));
        let other = ProofCache::open_at(&b, "v1".into(), true).expect("cache");
        assert_ne!(first.secret, other.secret);
        drop(fs::remove_dir_all(&a));
        drop(fs::remove_dir_all(&b));
    }

    #[test]
    fn a_damaged_secret_is_replaced_and_costs_only_misses() {
        let at = dir("damaged");
        let first = ProofCache::open_at(&at, "v1".into(), true).expect("cache");
        let key = first.key("tools", &QUERY);
        first.store(&key, "M", &[]);
        fs::write(at.join(super::SECRET_FILE), b"short").expect("damaged secret");
        let second = ProofCache::open_at(&at, "v1".into(), true).expect("cache");
        assert!(!second.holds(&key, "M", &[]));
        drop(fs::remove_dir_all(&at));
    }

    #[test]
    fn verify_deps_never_reads_but_still_stores() {
        let at = dir("verify_deps");
        let writer = ProofCache::open_at(&at, "v1".into(), false).expect("cache");
        let key = writer.key("tools", &QUERY);
        writer.store(&key, "M", &[]);
        assert!(!writer.holds(&key, "M", &[]));
        assert!(ProofCache::open_at(&at, "v1".into(), true).expect("cache").holds(&key, "M", &[]));
        drop(fs::remove_dir_all(&at));
    }
}
