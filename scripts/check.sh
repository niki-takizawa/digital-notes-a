#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

printf '%s\n' '==> rustfmt'
cargo fmt --all -- --check

printf '%s\n' '==> clippy'
cargo clippy --locked --all-targets -- -D warnings

printf '%s\n' '==> tests'
cargo test --locked --all-targets
