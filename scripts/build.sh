#!/usr/bin/env bash
# Build the native Rust engine and install the .node binary for local dev.
set -euo pipefail
cd "$(dirname "$0")/.."
source "$HOME/.cargo/env" 2>/dev/null || true

cargo build -p spectre-db-napi --release

mkdir -p build/Release
LIB="target/release/libspectre_db_napi.so"
case "$(uname -s)" in
  Darwin) LIB="target/release/libspectre_db_napi.dylib" ;;
  MINGW*|MSYS*|CYGWIN*) LIB="target/release/spectre_db_napi.dll" ;;
esac
cp "$LIB" "build/Release/spectre.db-rs.node"
echo "installed: build/Release/spectre.db-rs.node"
