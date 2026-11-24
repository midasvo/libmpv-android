#!/bin/bash -e
# Bundles the AAR. Gradle builds no native code: it packages libmpv/src/main/jniLibs,
# which scripts/player.sh fills for each ABI.

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
BUILD="$DIR/.."
MPV_ANDROID="$DIR/../.."

. $BUILD/include/path.sh
. $BUILD/include/depinfo.sh

if [ "$1" == "build" ]; then
	true
elif [ "$1" == "clean" ]; then
	rm -rf $MPV_ANDROID/{libmpv,.}/build
	exit 0
else
	exit 255
fi

for abi in armeabi-v7a arm64-v8a x86 x86_64; do
	[ -f "$MPV_ANDROID/libmpv/src/main/jniLibs/$abi/libplayer.so" ] ||
		echo >&2 "Warning: no libplayer.so for $abi in libmpv/src/main/jniLibs, the AAR will not support it"
done

# only the library: the root project also holds r8-test, a test app
./gradlew :libmpv:assembleRelease
