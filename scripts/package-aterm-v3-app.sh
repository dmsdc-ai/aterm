#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE_NAME="aterm-v3.app"
APP_DIR="/Applications/${BUNDLE_NAME}"
STAGE_DIR="${ROOT_DIR}/target/macos/${BUNDLE_NAME}"
CONTENTS_DIR="${STAGE_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

cd "${ROOT_DIR}"

cargo build --release

rm -rf "${STAGE_DIR}"
mkdir -p "${MACOS_DIR}" "${RESOURCES_DIR}"

cp "${ROOT_DIR}/src-v3/macos/Info.plist" "${CONTENTS_DIR}/Info.plist"
cp "${ROOT_DIR}/target/release/aterm-v3" "${MACOS_DIR}/aterm-v3"
cp "${ROOT_DIR}/src-tauri/icons/icon.icns" "${RESOURCES_DIR}/aterm-v3.icns"

chmod +x "${MACOS_DIR}/aterm-v3"

rm -rf "${APP_DIR}"
cp -R "${STAGE_DIR}" "${APP_DIR}"
