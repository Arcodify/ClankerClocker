#!/usr/bin/env bash

set -euo pipefail

APP_NAME="ClankerClocker.app"
SOURCE_PATH="${1:-}"
DEST_DIR="${HOME}/Applications"
DEST_PATH="${DEST_DIR}/${APP_NAME}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This installer is only for macOS." >&2
  exit 1
fi

if [[ -z "${SOURCE_PATH}" ]]; then
  if [[ -d "./src-tauri/target/release/bundle/macos/${APP_NAME}" ]]; then
    SOURCE_PATH="./src-tauri/target/release/bundle/macos/${APP_NAME}"
  else
    echo "Usage: npm run install:macos:unsigned -- /path/to/${APP_NAME}" >&2
    echo "Or run it from the repo root after building the app bundle." >&2
    exit 1
  fi
fi

if [[ ! -d "${SOURCE_PATH}" ]]; then
  echo "App bundle not found: ${SOURCE_PATH}" >&2
  exit 1
fi

mkdir -p "${DEST_DIR}"

echo "Installing ${APP_NAME} to ${DEST_PATH}"
rm -rf "${DEST_PATH}"
ditto "${SOURCE_PATH}" "${DEST_PATH}"

echo "Removing quarantine attributes from ${DEST_PATH}"
xattr -dr com.apple.quarantine "${DEST_PATH}" || true

echo "Launching ${APP_NAME}"
open "${DEST_PATH}"
