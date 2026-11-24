#!/bin/bash -e
# Builds the JNI wrapper, the Rust crate in libmpv/src/main/rust (a cdylib that becomes
# libplayer.so), for one ABI, then fills libmpv/src/main/jniLibs/<abi> with everything the
# AAR ships for that ABI: the shared libraries from the prefix (libmpv.so, FFmpeg),
# libplayer.so and the NDK's libc++_shared.so, all stripped. Gradle packages that
# directory as it is and builds no native code.
#
# Run by build.sh, which sets the ABI environment (loadarch) and runs this in the crate.
# Needs only $prefix_dir/lib/*.so from the native build, so CI can run it on the
# libraries handed over by the per-ABI jobs.

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
BUILD="$DIR/.."

. "$BUILD/include/depinfo.sh"
. "$BUILD/include/path.sh"

crate_dir="$( cd "$BUILD/../libmpv/src/main/rust" && pwd )"

case "$ndk_triple" in
	arm-linux-androideabi) rust_triple=armv7-linux-androideabi ;;
	aarch64-linux-android|i686-linux-android|x86_64-linux-android) rust_triple=$ndk_triple ;;
	*) echo "player.sh: unknown ndk_triple '$ndk_triple' (run it through build.sh)" >&2; exit 255 ;;
esac

if ! hash cargo 2>/dev/null; then
	echo "Error: cargo not found. Install rustup (https://rustup.rs); the toolchain and the" >&2
	echo "Android targets are pinned in libmpv/src/main/rust/rust-toolchain.toml." >&2
	exit 255
fi

if [ "$1" == "build" ]; then
	true
elif [ "$1" == "clean" ]; then
	cargo clean --manifest-path "$crate_dir/Cargo.toml" --target "$rust_triple"
	rm -rf "$native_dir"
	exit 0
else
	exit 255
fi

lib_dir="$prefix_dir/lib"
if [ ! -f "$lib_dir/libmpv.so" ]; then
	echo "Error: $lib_dir/libmpv.so not found; build mpv for this ABI first (./build.sh --arch <arch> mpv)" >&2
	exit 1
fi

# RUSTFLAGS (set below for the path remapping) replaces every rustflags entry of a
# .cargo/config.toml. Fail instead of silently dropping the crate's own flags.
for f in "$crate_dir/.cargo/config.toml" "$crate_dir/.cargo/config"; do
	if [ -f "$f" ] && grep -q 'rustflags' "$f"; then
		echo "Error: $f sets rustflags, which player.sh's RUSTFLAGS would override." >&2
		echo "Pass linker arguments from build.rs (cargo:rustc-cdylib-link-arg) instead." >&2
		exit 1
	fi
done

triple_env=$(echo "$rust_triple" | tr 'a-z-' 'A-Z_')
export "CARGO_TARGET_${triple_env}_LINKER=$(command -v "$CC")"
# C code compiled for the target (cc crate) uses the NDK compiler; build scripts and
# proc macros compiled for the host must not, so the plain CC/CXX/AR go away.
export "CC_${rust_triple//-/_}=$CC" "CXX_${rust_triple//-/_}=$CXX" "AR_${rust_triple//-/_}=llvm-ar"
unset CC CXX AR RANLIB LDFLAGS

# The crate's build script links against these (libmpv.so and libavcodec.so)
export LIBMPV_LIB_DIR="$lib_dir"

# Keep the checkout and cargo home paths out of the binary (panic locations, debug info)
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
export RUSTFLAGS="--remap-path-prefix=$crate_dir=/player --remap-path-prefix=$cargo_home=/cargo"

cargo build --release --locked --target "$rust_triple"

target_dir="${CARGO_TARGET_DIR:-$crate_dir/target}"
player="$target_dir/$rust_triple/release/libplayer.so"
libcxx="$toolchain/sysroot/usr/lib/$ndk_triple/libc++_shared.so"
for f in "$player" "$libcxx"; do
	[ -f "$f" ] || { echo "Error: $f not found" >&2; exit 1; }
done

# Start from an empty directory, so nothing stale (old symlinks, removed libraries) is packaged
rm -rf "$native_dir"
mkdir -p "$native_dir"
for f in "$lib_dir"/*.so "$player" "$libcxx"; do
	llvm-strip --strip-unneeded -o "$native_dir/$(basename "$f")" "$f"
done
# A fixed mode, so the AAR is the same whichever way the files got here (CI hands them
# between jobs as artifacts, which do not keep modes)
chmod 0644 "$native_dir"/*.so
ls -l "$native_dir"
