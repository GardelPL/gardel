#!/bin/bash
# Runs in the gardel-linux container (see Dockerfile): copies the read-only tree at /src to
# /work, then does what CI does on Windows. Arguments go to `cargo test` (a suite, a filter).
#   STAGES=build,verify,test   which steps to run (default: all three)
set -u
STAGES=${STAGES:-build,verify,test}
export GARDEL_CACHE_DIR=/tmp/gardel-cache   # CI proves everything itself
rm -rf /work && mkdir -p /work
# The build outputs, the Windows tools and anything not part of the tree stay out.
tar -C /src --exclude=./.git --exclude=./gardelc/target --exclude='*/build' --exclude=./tools/dafny \
    --exclude='*.exe' --exclude=./unknow.key.txt -cf - . | tar -C /work -xf -
cd /work || exit 2
echo "gcc: $(gcc --version | head -1)"
echo "rustc: $(rustc --version)"
echo "z3: $($Z3_EXE --version)"
status=0

if [[ $STAGES == *build* ]]; then
  echo "== build"
  (cd gardelc && cargo build --locked) || exit 1
fi

if [[ $STAGES == *verify* ]]; then
  echo "== verify the stdlib and examples"
  g=/work/gardelc/target/debug/gardelc
  [ -n "${CARGO_TARGET_DIR:-}" ] && g=$CARGO_TARGET_DIR/debug/gardelc
  targets=(--stdlib)
  for f in examples/*.gdl; do targets+=("examples/$(basename "$f")"); done
  for d in examples/*/; do targets+=("examples/$(basename "$d")"); done
  for t in "${targets[@]}"; do
    out=$("$g" "$t" --strict 2>&1)
    code=$?
    printf '  %-28s exit %s  %s\n' "$t" "$code" "$(echo "$out" | grep -h 'errors, ' | tail -1 | sed 's/ — .*//')"
    if [ $code -ne 0 ]; then
      status=1
      echo "$out" | grep -E '\[ERROR\]|error|warning: ' | head -20 | sed 's/^/      /'
    fi
  done
fi

if [[ $STAGES == *test* ]]; then
  echo "== cargo test"
  (cd gardelc && cargo test --locked --no-fail-fast "$@") || status=1
fi
exit $status
