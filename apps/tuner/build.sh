#!/bin/bash
#
# build.sh — one-shot build chain for the ProsodiaTuner harness.
#
# Chains the two build systems in the right order: regenerates the Rust FFI
# xcframeworks (macos-arm64 slice) via the repo-root build_frameworks.sh, then
# builds the macOS app with the correct scheme and destination. Extra arguments
# are passed through to xcodebuild (e.g. `./build.sh clean build`).
#
# The Xcode project also carries a "Check FFI Framework Freshness" tripwire
# phase, so a GUI build that skips this script fails loudly instead of linking
# stale Rust.

set -euo pipefail

cd "$(dirname "$0")"

../../build_frameworks.sh

exec xcodebuild \
    -project ProsodiaTuner.xcodeproj \
    -scheme ProsodiaTuner \
    -destination "platform=macOS,arch=arm64" \
    "${@:-build}"
