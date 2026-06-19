#!/usr/bin/env bash
#
# build_android.sh — Compile Rust crates for Android (aarch64 and x86_64) and generate Kotlin UniFFI bindings.
#

set -euo pipefail

cd "$(dirname "$0")"
ROOT="$(pwd)"
OUT="$ROOT/platforms/android"
GEN_KOTLIN="$OUT/src/main/kotlin"
JNILIBS="$OUT/src/main/jniLibs"

# NDK setup
NDK_VERSION="26.1.10909125"
API=26

HOST_OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$HOST_OS" in
    darwin)
        DEFAULT_NDK="/Users/lmcfarlin/Library/Android/sdk/ndk/$NDK_VERSION"
        PREBUILT_ARCH="darwin-x86_64"
        HOST_LIB_EXT="dylib"
        DEFAULT_SDK_DIR="/Users/lmcfarlin/Library/Android/sdk"
        ;;
    linux)
        DEFAULT_NDK="/home/lmcfarlin/Android/Sdk/ndk/$NDK_VERSION"
        PREBUILT_ARCH="linux-x86_64"
        HOST_LIB_EXT="so"
        DEFAULT_SDK_DIR="/home/lmcfarlin/Android/Sdk"
        ;;
    *)
        echo "Unsupported OS: $HOST_OS"
        exit 1
        ;;
esac

ANDROID_NDK="${ANDROID_NDK_HOME:-$DEFAULT_NDK}"
ANDROID_SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$DEFAULT_SDK_DIR}}"

if [ ! -d "$ANDROID_NDK" ]; then
    echo "Error: Android NDK not found at $ANDROID_NDK."
    echo "Please set ANDROID_NDK_HOME or ensure the NDK is installed."
    exit 1
fi

echo "==> Using Android NDK at $ANDROID_NDK"

# Write local.properties dynamically for the current machine
echo "==> Updating local.properties for the build..."
cat << EOF > "$OUT/../../platforms/android/local.properties"
sdk.dir=$ANDROID_SDK
ndk.dir=$ANDROID_NDK
EOF

cat << EOF > "$OUT/../../apps/android-reader/local.properties"
# Local configuration properties.
sdk.dir=$ANDROID_SDK
ndk.dir=$ANDROID_NDK
EOF

# Add Rust targets if missing
echo "==> Verifying rustup targets..."
rustup target add aarch64-linux-android x86_64-linux-android

# Toolchain prebuilt bin path
TOOLCHAIN_BIN="$ANDROID_NDK/toolchains/llvm/prebuilt/$PREBUILT_ARCH/bin"

# Export compilation flags for NDK
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN_BIN/aarch64-linux-android$API-clang"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$TOOLCHAIN_BIN/x86_64-linux-android$API-clang"

# Map: ABI -> rust target
declare -A TARGETS=(
    ["arm64-v8a"]="aarch64-linux-android"
    ["x86_64"]="x86_64-linux-android"
)

CRATES=(folioparser stage director actor)

# Ensure host release libraries are built for bindgen to parse
echo "==> Building host Rust crates (release) for metadata binding generation…"
cargo build --release

# Clean and recreate outputs
rm -rf "$JNILIBS"
mkdir -p "$JNILIBS/arm64-v8a" "$JNILIBS/x86_64" "$GEN_KOTLIN"

# Build Rust libraries for Android
for abi in "${!TARGETS[@]}"; do
    target="${TARGETS[$abi]}"
    echo "==> Building Rust crates for $abi ($target)…"
    
    cargo build --target "$target" --release -p folioparser -p stage -p director -p actor
    
    for crate in "${CRATES[@]}"; do
        # Copy compiled .so file
        src_so="target/$target/release/lib${crate}.so"
        dst_so="$JNILIBS/$abi/lib${crate}.so"
        echo "Copying $src_so to $dst_so"
        cp "$src_so" "$dst_so"
    done
done

# Generate Kotlin bindings from the built host release binaries
echo "==> Generating Kotlin UniFFI bindings..."
for crate in "${CRATES[@]}"; do
    echo "Generating bindings for $crate..."
    cargo run -q -p stage --bin uniffi-bindgen -- \
        generate --library "target/release/lib${crate}.${HOST_LIB_EXT}" \
        --language kotlin --out-dir "$GEN_KOTLIN"
done

echo "==> Done. Android JNI libs and Kotlin bindings written to platforms/android/"
