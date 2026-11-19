#!/bin/bash
# Runs gardelc's checks on Linux, in Docker, from this tree as it is now (P8). Works from
# Git Bash on Windows and from a Linux or macOS shell.
#   scripts/linux/test.sh                 build, verify the stdlib and examples, all tests
#   scripts/linux/test.sh --test connect  only the `connect` suite (arguments go to cargo test)
#   STAGES=build,test scripts/linux/test.sh ...   skip a step
# The cargo target directory is kept in the volume `gardel-linux-target` between runs.
set -u
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
IMAGE=${GARDEL_LINUX_IMAGE:-gardel-linux}
# GARDEL_LINUX_BASE: an Ubuntu 24.04 image with gcc, for where apt cannot reach its mirrors.
docker image inspect "$IMAGE" > /dev/null 2>&1   || docker build ${GARDEL_LINUX_BASE:+--build-arg BASE=$GARDEL_LINUX_BASE} -t "$IMAGE" "$ROOT/scripts/linux" || exit 2
src=$ROOT
command -v cygpath > /dev/null && src=$(cygpath -w "$ROOT")
MSYS_NO_PATHCONV=1 exec docker run --rm \
  -v "$src:/src:ro" -v gardel-linux-target:/target -e CARGO_TARGET_DIR=/target -e STAGES="${STAGES:-build,verify,test}" \
  "$IMAGE" bash /src/scripts/linux/inside.sh "$@"
