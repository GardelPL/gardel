//! Parallel balanced tree sequence benchmark in idiomatic Rust.
//!
//! Mirrors Gardel's `SEQ[U64]` tree representation:
//! - `Empty`
//! - `Leaf(u64)`
//! - `Concat { left, right, len }`

use std::env;
use std::time::Instant;

/// Parallel balanced tree sequence holding `u64` values.
#[derive(Debug)]
pub enum Seq {
    Empty,
    Leaf(u64),
    Concat {
        left: Box<Seq>,
        right: Box<Seq>,
        len: u64,
    },
}

impl Seq {
    /// Constructs an empty sequence.
    #[inline]
    #[must_use]
    pub const fn empty() -> Self {
        Seq::Empty
    }

    /// Constructs a single-element sequence.
    #[inline]
    #[must_use]
    pub const fn singleton(x: u64) -> Self {
        Seq::Leaf(x)
    }

    /// Returns the length (element count) of the sequence.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> u64 {
        match self {
            Seq::Empty => 0,
            Seq::Leaf(_) => 1,
            Seq::Concat { len, .. } => *len,
        }
    }

    /// Returns `true` if the sequence contains no elements.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Concatenates two sequences in O(1) time.
    #[must_use]
    pub fn concat(left: Self, right: Self) -> Self {
        match (left, right) {
            (Seq::Empty, r) => r,
            (l, Seq::Empty) => l,
            (l, r) => {
                let len = l.len() + r.len();
                Seq::Concat {
                    left: Box::new(l),
                    right: Box::new(r),
                    len,
                }
            }
        }
    }

    /// Recursively builds a balanced binary tree for the range `lo..=hi`.
    ///
    /// Uses divide-and-conquer parallelism via `rayon::join` when the interval exceeds 512.
    pub fn from_range(lo: u64, hi: u64) -> Self {
        if lo > hi {
            return Seq::Empty;
        }
        if lo == hi {
            return Seq::singleton(lo);
        }
        let mid = lo + (hi - lo) / 2;
        if hi - lo > 512 {
            let (left, right) = rayon::join(
                || Seq::from_range(lo, mid),
                || Seq::from_range(mid + 1, hi),
            );
            Seq::concat(left, right)
        } else {
            let left = Seq::from_range(lo, mid);
            let right = Seq::from_range(mid + 1, hi);
            Seq::concat(left, right)
        }
    }

    /// Computes the sum of all elements in the tree.
    ///
    /// Uses divide-and-conquer parallelism via `rayon::join` when subtree length exceeds 512.
    pub fn parallel_sum(&self) -> u64 {
        match self {
            Seq::Empty => 0,
            Seq::Leaf(val) => *val,
            Seq::Concat { left, right, len } => {
                if *len > 512 {
                    let (sl, sr) = rayon::join(
                        || left.parallel_sum(),
                        || right.parallel_sum(),
                    );
                    sl + sr
                } else {
                    left.parallel_sum() + right.parallel_sum()
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let n: u64 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000);

    let t0 = Instant::now();
    let tree = Seq::from_range(1, n);
    let t_build = t0.elapsed();

    let t1 = Instant::now();
    let total = tree.parallel_sum();
    let t_sum = t1.elapsed();

    let expected = n * (n + 1) / 2;
    assert_eq!(total, expected, "Checksum mismatch");

    let build_ms = t_build.as_secs_f64() * 1000.0;
    let sum_ms = t_sum.as_secs_f64() * 1000.0;
    let total_ms = build_ms + sum_ms;

    println!(
        "RESULT build_ms={build_ms:.2} sum_ms={sum_ms:.2} total_ms={total_ms:.2} sum={total}"
    );

    // Drop happens here automatically and deterministically.
}

