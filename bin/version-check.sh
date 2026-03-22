#!/bin/bash
# Quick version info check — no build required
# Usage: ./bin/version-check.sh

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
HASH=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
DATE=$(date +%Y-%m-%d)
BUILD=$(cat .build-number 2>/dev/null || echo "0")
NEXT_BUILD=$((BUILD + 1))

if git diff --quiet 2>/dev/null; then
  DIRTY=""
else
  DIRTY="*"
fi

echo "aterm v3 — ${VERSION}-${HASH}${DIRTY} build.${NEXT_BUILD} (${DATE})"
