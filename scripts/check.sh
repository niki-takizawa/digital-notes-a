#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

printf '%s\n' '==> rustfmt'
cargo fmt --all -- --check

printf '%s\n' '==> clippy'
cargo clippy --locked --all-targets -- -D warnings

printf '%s\n' '==> tests'
cargo test --locked --all-targets

printf '%s\n' '==> schema contract tests'
"${SCHEMA_TEST_PYTHON:-python3}" -m unittest discover -s tests -p 'test_schema_contract.py'
