#!/usr/bin/env bash
set -euo pipefail

REF="${1:-main}"
REPO_URL="${ENS_NORMALIZE_JS_REPO:-https://github.com/adraffy/ens-normalize.js.git}"
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
WORKDIR="$(mktemp -d)"

cleanup() {
	rm -rf "$WORKDIR"
}
trap cleanup EXIT

echo "Cloning $REPO_URL#$REF"
mkdir -p "$WORKDIR/ens-normalize.js"
git -C "$WORKDIR/ens-normalize.js" init
git -C "$WORKDIR/ens-normalize.js" remote add origin "$REPO_URL"
git -C "$WORKDIR/ens-normalize.js" fetch --depth 1 origin "$REF"
git -C "$WORKDIR/ens-normalize.js" checkout --detach FETCH_HEAD

pushd "$WORKDIR/ens-normalize.js" >/dev/null
npm ci
npm test
popd >/dev/null

cp "$WORKDIR/ens-normalize.js/derive/output/spec.json" "$ROOT/data/spec.json"
cp "$WORKDIR/ens-normalize.js/derive/output/nf.json" "$ROOT/data/nf.json"
cp "$WORKDIR/ens-normalize.js/derive/output/nf-tests.json" "$ROOT/tests/fixtures/nf-tests.json"
cp "$WORKDIR/ens-normalize.js/validate/tests.json" "$ROOT/tests/fixtures/validate-tests.json"
cp "$WORKDIR/ens-normalize.js/validate/custom-tests.json" "$ROOT/tests/fixtures/custom-tests.json"

pushd "$ROOT" >/dev/null
cargo fmt
cargo test
popd >/dev/null

echo "Updated upstream ENS normalize data from $REF"
