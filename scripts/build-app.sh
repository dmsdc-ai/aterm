#!/usr/bin/env bash
# Build build/aterm.app. The build itself is `make app` (npm/prepare-platform-package.mjs
# shells out to the same target) — this script only covers the prerequisite the Makefile
# does not: since Xcode 26 the Metal compiler is a separately downloaded component, so
# `xcrun metal` fails on a fresh machine with "missing Metal Toolchain".
#
# Install it with:  xcodebuild -downloadComponent MetalToolchain
set -euo pipefail
cd "$(dirname "$0")/.."

if xcrun -sdk macosx metal --version >/dev/null 2>&1; then
  exec make app
fi

echo "[build-app] Metal toolchain not installed — cannot compile macos/Sources/Shaders.metal" >&2
if [ ! -f build/default.metallib ]; then
  echo "[build-app] and no build/default.metallib to reuse. Run: xcodebuild -downloadComponent MetalToolchain" >&2
  exit 1
fi

# ponytail: reuse the cached metallib only when it is newer than the shader source it was
# compiled from; anything else is a stale-shader bug waiting to happen.
if [ macos/Sources/Shaders.metal -nt build/default.metallib ]; then
  echo "[build-app] build/default.metallib is older than Shaders.metal — refusing to ship stale shaders." >&2
  echo "[build-app] Run: xcodebuild -downloadComponent MetalToolchain" >&2
  exit 1
fi

echo "[build-app] reusing existing build/default.metallib (shaders unchanged since it was compiled)" >&2
make -o metal app
