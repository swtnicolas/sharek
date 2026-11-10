#!/bin/sh
set -eu
repo=$(cd "$(dirname "$0")/.." && pwd)
cargo test --release --workspace --manifest-path "$repo/Cargo.toml"
cargo clippy --release --workspace --all-targets --manifest-path "$repo/Cargo.toml" -- -D warnings
