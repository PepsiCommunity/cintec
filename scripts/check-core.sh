#!/usr/bin/env bash
# Run the dependency-free policy tests, including in an offline sandbox.
set -euo pipefail
cd "$(dirname "$0")/.."
build_dir=$(mktemp -d)
trap 'rm -rf "$build_dir"' EXIT
for module in library recovery selection; do
  rustc --edition=2024 --test "src/$module.rs" -o "$build_dir/$module-tests"
  "$build_dir/$module-tests"
done
