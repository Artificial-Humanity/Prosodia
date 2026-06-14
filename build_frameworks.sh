#!/bin/bash
#
# build_frameworks.sh — Regenerate UniFFI bindings and build the FFI xcframeworks.
#
# Produces FRAMEWORK-STYLE xcframeworks (modulemap lives inside
# <Module>.framework/Modules/), not static-library xcframeworks. Static-library
# xcframeworks each ship Headers/module.modulemap, which Xcode flattens into one
# shared $(BUILT_PRODUCTS_DIR)/include/ directory — causing
# "Multiple commands produce .../include/module.modulemap". Framework-style
# packaging keeps each modulemap namespaced inside its framework, so they never
# collide.
#
# The Tuner is macOS-only, so we build the macos-arm64 slice. Add more `cargo
# build --target ...` + lipo/`-framework` slices here for iOS/visionOS later.

set -euo pipefail

cd "$(dirname "$0")"
ROOT="$(pwd)"
OUT="$ROOT/platforms/apple"
GEN_SWIFT="$OUT/Sources/Kit/Generated"
FFI_HEADERS="$OUT/FFIHeaders"
STAGING="$ROOT/.build_frameworks"

# Map: cargo crate name -> UniFFI module/header base is "<crate>FFI".
CRATES=(folioparser stage director actor)

echo "==> Building Rust crates (release)…"
cargo build --release

rm -rf "$STAGING"
mkdir -p "$STAGING" "$GEN_SWIFT"

for crate in "${CRATES[@]}"; do
    module="${crate}FFI"
    echo "==> Processing $crate (module: $module)…"

    # 1. Regenerate Swift bindings + C header + modulemap from the built dylib.
    bindgen_dir="$STAGING/bindgen-$crate"
    mkdir -p "$bindgen_dir"
    cargo run -q -p stage --bin uniffi-bindgen -- \
        generate --library "target/release/lib${crate}.dylib" \
        --language swift --out-dir "$bindgen_dir"

    # 2. Assemble a deep macOS framework bundle.
    fw="$STAGING/$module.framework"
    rm -rf "$fw"
    mkdir -p "$fw/Versions/A/Headers" "$fw/Versions/A/Modules" "$fw/Versions/A/Resources"
    
    # Create standard macOS framework symlinks
    cd "$fw"
    ln -sf A Versions/Current
    ln -sf Versions/Current/Headers Headers
    ln -sf Versions/Current/Modules Modules
    ln -sf Versions/Current/Resources Resources
    ln -sf Versions/Current/${module} ${module}
    cd "$ROOT"

    cp "$bindgen_dir/${module}.h" "$fw/Versions/A/Headers/${module}.h"
    # Framework modulemap: the binding's plain `module X {…}` becomes a
    # `framework module X {…}` discovered via the framework's Modules dir.
    cat > "$fw/Versions/A/Modules/module.modulemap" <<EOF
framework module ${module} {
    header "${module}.h"
    export *
}
EOF
    # The framework binary IS the Rust dynamic library, named after the module.
    cp "target/release/lib${crate}.dylib" "$fw/Versions/A/${module}"

    cat > "$fw/Versions/A/Resources/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleExecutable</key><string>${module}</string>
    <key>CFBundleIdentifier</key><string>technology.mcfarlin.${module}</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>${module}</string>
    <key>CFBundlePackageType</key><string>FMWK</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>CFBundleSupportedPlatforms</key><array><string>MacOSX</string></array>
    <key>MinimumOSVersion</key><string>14.0</string>
</dict>
</plist>
EOF

    # 3. Wrap the framework in an xcframework (replacing the old static-lib one).
    rm -rf "$OUT/$module.xcframework"
    xcodebuild -create-xcframework -framework "$fw" -output "$OUT/$module.xcframework"

    # 4. Publish the generated Swift bindings into the Kit target.
    cp "$bindgen_dir/${crate}.swift" "$GEN_SWIFT/${crate}.swift"

    # 5. Refresh the (reference) FFIHeaders copy so it isn't stale.
    mkdir -p "$FFI_HEADERS/$module"
    cp "$bindgen_dir/${module}.h" "$FFI_HEADERS/$module/${module}.h"
    cp "$fw/Modules/module.modulemap" "$FFI_HEADERS/$module/module.modulemap"
done

rm -rf "$STAGING"
echo "==> Done. Framework-style xcframeworks written to $OUT/*.xcframework"
