#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v wasm-pack >/dev/null 2>&1; then
	echo "wasm-pack is required for JS/WASM compatibility tests." >&2
	echo "Install it with: cargo install wasm-pack --locked" >&2
	exit 127
fi

pushd "$ROOT" >/dev/null
rustup target add wasm32-unknown-unknown
wasm-pack build --target nodejs --out-dir tests/js-compat/pkg --features wasm
npm --prefix tests/js-compat test
popd >/dev/null
