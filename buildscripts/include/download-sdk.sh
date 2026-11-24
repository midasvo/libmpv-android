#!/bin/bash -e
# Installs the Android SDK packages (platform, build tools, NDK) into sdk/android-sdk-<os>
# and fetches gas-preprocessor into sdk/bin.
#
#   IN_CI=1            reuse the runner's preinstalled SDK ($ANDROID_HOME) via a symlink
#   SDK_WITHOUT_NDK=1  skip the NDK (Gradle-only jobs: packaging and tests need no NDK)

. ./include/depinfo.sh

host_sdk=${ANDROID_HOME:-} # path.sh points ANDROID_HOME at sdk/android-sdk-<os>
. ./include/path.sh # load $os var

[ -z "$IN_CI" ] && IN_CI=0 # running on a CI runner?
[ -z "$WGET" ] && WGET=wget # possibility of calling wget differently
[ -z "$SDK_WITHOUT_NDK" ] && SDK_WITHOUT_NDK=0

if ! javac -version &>/dev/null; then
	echo "Error: missing Java Development Kit."
	if [ "$os" == "linux" ]; then
		hash yum &>/dev/null && \
			echo "Install it using e.g. sudo yum install java-latest-openjdk-devel"
		apt-get -v &>/dev/null && \
			echo "Install it using e.g. sudo apt-get install default-jdk-headless"
	fi
	exit 255
fi

mkdir -p sdk && cd sdk

if [ $IN_CI -eq 1 ] && [ ! -e "android-sdk-${os}" ] && [ -n "$host_sdk" ]; then
	ln -s "$host_sdk" "android-sdk-${os}"
fi

# Android SDK
if [ ! -d "android-sdk-${os}" ]; then
	$WGET "https://dl.google.com/android/repository/commandlinetools-${os}-${v_sdk}.zip"
	mkdir "android-sdk-${os}"
	unzip -q -d "android-sdk-${os}" "commandlinetools-${os}-${v_sdk}.zip"
	rm "commandlinetools-${os}-${v_sdk}.zip"
fi
sdk_install () {
	local pkgs=("platforms/${v_platform}" "build-tools/${v_sdk_build_tools}")
	[ $SDK_WITHOUT_NDK -eq 1 ] || pkgs+=("ndk/${v_ndk}")

	local exe="./android-sdk-$os/cmdline-tools/latest/bin/android"
	[ -x "$exe" ] || exe="./android-sdk-$os/cmdline-tools/bin/android"
	if [ -x "$exe" ]; then
		"$exe" --no-metrics --sdk="${ANDROID_HOME}" sdk install "${pkgs[@]}"
		return
	fi

	# Fall back to the legacy sdkmanager (e.g. the preinstalled SDK on CI runners)
	exe="./android-sdk-$os/cmdline-tools/latest/bin/sdkmanager"
	[ -x "$exe" ] || exe="./android-sdk-$os/cmdline-tools/bin/sdkmanager"
	echo y | "$exe" --sdk_root="${ANDROID_HOME}" "${pkgs[@]//\//;}"
}
sdk_install

# gas-preprocessor
mkdir -p bin
$WGET "https://github.com/FFmpeg/gas-preprocessor/raw/${v_gas_preprocessor}/gas-preprocessor.pl" \
	-O bin/gas-preprocessor.pl
chmod +x bin/gas-preprocessor.pl

cd ..
