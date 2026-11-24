#!/bin/bash
# Runs the instrumented tests on one emulator or device, without CI and without the native
# build: the native libraries (libmpv.so, FFmpeg) come from a published AAR, the wrapper
# (libplayer.so) is built from this checkout with cargo and the NDK's clang, and Gradle runs
# the connected tests against the device you name. Works in Git Bash on Windows and on
# Linux and macOS.
#
#   local-test.sh --serial <serial> [--aar <path> | --release <version>] [--r8] [-- <gradle args>]
#
#   --serial <serial>   the device to test on, as `adb devices` lists it (or set ANDROID_SERIAL).
#                       Required: a phone of your own may be attached too.
#   --aar <path>        take the native libraries from this AAR
#   --release <version> take them from the AAR of this GitHub release (default: the latest
#                       release), downloaded with `gh release download`
#   --r8                also run the R8 test app (:r8-test:connectedReleaseAndroidTest)
#   -- <gradle args>    passed to Gradle after the tasks
#
# What it needs, and does not install: adb, the Android SDK with the NDK version in
# include/depinfo.sh (found through ANDROID_HOME, ANDROID_SDK_ROOT or sdk.dir in
# local.properties), a JDK, rustup (cargo installs the toolchain and Android targets pinned in
# rust-toolchain.toml on first use), unzip, and gh when no --aar is given. It does not start an
# emulator or install SDK packages.
#
# libmpv/src/main/jniLibs is moved aside while the tests run and put back afterwards, so a
# build of your own there is kept. The native libraries are those of the AAR you chose: a test
# that depends on a native change made after that release (an mpv patch, a new FFmpeg) can fail
# here and pass in CI, which builds them from source.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$here/.." && pwd)
repo=midasvo/libmpv-android
api_level=26 # minSdk, as build.sh's loadarch

die () { echo "local-test.sh: $*" >&2; exit 1; }
usage () { sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "$0"; exit "${1:-0}"; }

serial=${ANDROID_SERIAL:-} aar= release= r8=0
gradle_args=()
while [ $# -gt 0 ]; do
	case "$1" in
		--serial) serial=${2:?--serial needs a value}; shift ;;
		--aar) aar=${2:?--aar needs a path}; shift ;;
		--release) release=${2:?--release needs a version}; shift ;;
		--r8) r8=1 ;;
		-h|--help) usage 0 ;;
		--) shift; gradle_args=("$@"); break ;;
		*) echo "local-test.sh: unknown argument $1" >&2; usage 2 ;;
	esac
	shift
done

[ -n "$serial" ] || die "name the device to test on: --serial <serial> or ANDROID_SERIAL (see adb devices)"
[ -z "$aar" ] || [ -z "$release" ] || die "give --aar or --release, not both"

case "$(uname -s)" in
	MINGW*|MSYS*|CYGWIN*) host=windows-x86_64 windows=1 ;;
	Darwin) host=darwin-x86_64 windows=0 ;;
	Linux) host=linux-x86_64 windows=0 ;;
	*) die "unsupported host $(uname -s)" ;;
esac
native_path () { if [ "$windows" = 1 ]; then cygpath -w "$1"; else printf '%s' "$1"; fi; }

# ------------------------------------------------------------------ the SDK and the device

sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}
if [ -z "$sdk" ] && [ -f "$repo_root/local.properties" ]; then
	sdk=$(sed -n 's/^sdk\.dir=//p' "$repo_root/local.properties" | tr -d '\r' | sed 's/\\:/:/g; s/\\\\/\//g')
fi
[ -n "$sdk" ] || die "no Android SDK: set ANDROID_HOME, or sdk.dir in local.properties"
[ "$windows" = 1 ] && sdk=$(cygpath -u "$sdk")
[ -d "$sdk" ] || die "Android SDK $sdk does not exist"

adb=adb
hash adb 2>/dev/null || adb=$sdk/platform-tools/adb
"$adb" version > /dev/null 2>&1 || die "adb not found (on PATH or in $sdk/platform-tools)"

state=$("$adb" -s "$serial" get-state 2>/dev/null | tr -d '\r') || true
[ "$state" = device ] || die "device $serial is not ready (state: '${state:-not found}'); see adb devices"
abi=$("$adb" -s "$serial" shell getprop ro.product.cpu.abi | tr -d '\r')
sdk_int=$("$adb" -s "$serial" shell getprop ro.build.version.sdk | tr -d '\r')
case "$abi" in
	armeabi-v7a) rust_triple=armv7-linux-androideabi ndk_triple=arm-linux-androideabi clang=armv7a-linux-androideabi$api_level ;;
	arm64-v8a) rust_triple=aarch64-linux-android ndk_triple=aarch64-linux-android clang=aarch64-linux-android$api_level ;;
	x86) rust_triple=i686-linux-android ndk_triple=i686-linux-android clang=i686-linux-android$api_level ;;
	x86_64) rust_triple=x86_64-linux-android ndk_triple=x86_64-linux-android clang=x86_64-linux-android$api_level ;;
	*) die "device $serial has ABI '$abi', which this library does not build" ;;
esac
echo "Device $serial: API $sdk_int, $abi"

# shellcheck source=include/depinfo.sh
. "$here/include/depinfo.sh"
toolchain=$sdk/ndk/$v_ndk/toolchains/llvm/prebuilt/$host
[ -d "$toolchain" ] || die "NDK $v_ndk (v_ndk in include/depinfo.sh) is not installed in $sdk/ndk; install it with the SDK manager"
exe=
[ "$windows" = 1 ] && exe=.exe
linker=$toolchain/bin/$clang-clang
[ "$windows" = 1 ] && linker=$linker.cmd
strip=$toolchain/bin/llvm-strip$exe
libcxx=$toolchain/sysroot/usr/lib/$ndk_triple/libc++_shared.so
for f in "$linker" "$strip" "$libcxx"; do
	[ -f "$f" ] || die "$f not found in NDK $v_ndk"
done

for tool in cargo unzip; do
	hash "$tool" 2>/dev/null || die "'$tool' is required"
done

work=$(mktemp -d)
jnilibs=$repo_root/libmpv/src/main/jniLibs
backup=$work/jniLibs.orig
restore () {
	local status=$?
	if [ -d "$backup" ]; then
		rm -rf "$jnilibs"
		cp -a "$backup" "$jnilibs"
		echo "Put libmpv/src/main/jniLibs back as it was"
	elif [ -n "${created_jnilibs:-}" ]; then
		rm -rf "$jnilibs"
	fi
	rm -rf "$work"
	exit "$status"
}
trap restore EXIT

# ------------------------------------------------------------------ the native libraries

if [ -z "$aar" ]; then
	hash gh 2>/dev/null || die "gh is needed to download a release (or pass --aar <path>)"
	if [ -z "$release" ]; then
		release=$(gh release view -R "$repo" --json tagName --jq .tagName) || die "could not find the latest release of $repo"
	fi
	release=v${release#v}
	echo "Downloading the AAR of $release"
	gh release download "$release" -R "$repo" -p 'libmpv-*.aar' -D "$work/release" ||
		die "could not download the AAR of release $release"
	aar=$(find "$work/release" -name '*.aar' | head -n 1)
fi
[ -f "$aar" ] || die "AAR $aar not found"
unzip -q "$aar" "jni/$abi/*" -d "$work/aar" || die "$aar has no libraries for $abi"
lib_dir=$work/aar/jni/$abi
[ -f "$lib_dir/libmpv.so" ] || die "$aar has no jni/$abi/libmpv.so"
echo "Native libraries from $aar"

# ------------------------------------------------------------------ the wrapper

# As scripts/player.sh does in CI: cargo links with the NDK's clang against this ABI's
# libmpv.so and libavcodec.so (the crate's build.rs reads LIBMPV_LIB_DIR).
crate_dir=$repo_root/libmpv/src/main/rust
linker_var=CARGO_TARGET_$(echo "$rust_triple" | tr 'a-z-' 'A-Z_')_LINKER
echo "Building libplayer.so for $rust_triple"
(
	cd "$crate_dir"
	export "$linker_var=$(native_path "$linker")"
	export LIBMPV_LIB_DIR
	LIBMPV_LIB_DIR=$(native_path "$lib_dir")
	cargo build --release --locked --target "$rust_triple"
)
player=${CARGO_TARGET_DIR:-$crate_dir/target}/$rust_triple/release/libplayer.so
[ -f "$player" ] || die "cargo did not produce $player"

# ------------------------------------------------------------------ jniLibs

if [ -e "$jnilibs" ]; then
	cp -a "$jnilibs" "$backup"
	rm -rf "$jnilibs"
fi
created_jnilibs=1
mkdir -p "$jnilibs/$abi"
for f in "$lib_dir"/*.so; do
	case "${f##*/}" in libplayer.so|libc++_shared.so) continue ;; esac
	cp "$f" "$jnilibs/$abi/"
done
"$strip" --strip-unneeded -o "$(native_path "$jnilibs/$abi/libplayer.so")" "$(native_path "$player")"
"$strip" --strip-unneeded -o "$(native_path "$jnilibs/$abi/libc++_shared.so")" "$(native_path "$libcxx")"
chmod 0644 "$jnilibs/$abi"/*.so
ls -l "$jnilibs/$abi"

# ------------------------------------------------------------------ the tests

tasks=(:libmpv:connectedDebugAndroidTest)
[ "$r8" = 1 ] && tasks+=(:r8-test:connectedReleaseAndroidTest)
results=("$repo_root/libmpv/build/outputs/androidTest-results")
[ "$r8" = 1 ] && results+=("$repo_root/r8-test/build/outputs/androidTest-results")
for d in "${results[@]}"; do rm -rf "$d"; done

echo "Running ${tasks[*]} on $serial"
export ANDROID_SERIAL=$serial
gradle_status=0
(cd "$repo_root" && ./gradlew "${tasks[@]}" --no-configuration-cache ${gradle_args[@]+"${gradle_args[@]}"}) || gradle_status=$?

# Gradle can succeed when the instrumentation crashed before a test ran: the reports decide
summary_status=0
bash "$here/test-summary.sh" header
bash "$here/test-summary.sh" junit "connectedDebugAndroidTest on $serial (API $sdk_int, $abi)" "${results[0]}" || summary_status=1
if [ "$r8" = 1 ]; then
	bash "$here/test-summary.sh" junit "r8-test on $serial (API $sdk_int, $abi)" "${results[1]}" || summary_status=1
fi
echo "Reports: libmpv/build/reports/androidTests/connected/"

[ "$gradle_status" = 0 ] || exit "$gradle_status"
exit "$summary_status"
