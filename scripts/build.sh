#!/bin/sh
# Build the console binary where herdr-plugin.toml expects it (./bin/broadcast-pane).
set -eu
cd "$(dirname "$0")/.."
cargo build --release
mkdir -p bin
cp target/release/broadcast-pane bin/broadcast-pane
