#!/bin/bash -e
# Installs the host tools the native build needs (autotools, ninja, nasm, ...) and the
# pinned meson (v_meson in depinfo.sh). Run by download.sh, also in CI (IN_CI=1, where
# meson is installed system-wide and a version mismatch is an error).

. ./include/depinfo.sh
. ./include/path.sh # load $os var

[ -z "$IN_CI" ] && IN_CI=0

if [ "$os" == "linux" ]; then
	if hash yum &>/dev/null; then
		sudo yum install autoconf pkgconfig libtool ninja-build \
			python3-pip python3-setuptools unzip wget gperf nasm
		python3 -m pip install "meson==$v_meson" jsonschema jinja2
	elif apt-get -v &>/dev/null; then
		sudo apt-get update
		sudo apt-get install -y autoconf pkgconf libtool ninja-build nasm \
			python3-pip python3-setuptools unzip python3-jinja2 gperf wget
		if [ $IN_CI -eq 1 ]; then
			sudo python3 -m pip install --break-system-packages "meson==$v_meson"
		else
			python3 -m pip install "meson==$v_meson" ||
				python3 -m pip install --break-system-packages "meson==$v_meson"
		fi
	else
		echo "Warning: no yum or apt-get; install autoconf, pkg-config, libtool, ninja, nasm, gperf and meson $v_meson yourself" >&2
	fi
elif [ "$os" == "mac" ]; then
	if ! hash brew 2>/dev/null; then
		echo "Error: brew not found. You need to install Homebrew: https://brew.sh/"
		exit 255
	fi
	brew install \
		automake autoconf libtool pkg-config \
		coreutils gnu-sed wget meson ninja gperf nasm
fi

have_meson=$(meson --version 2>/dev/null || true)
if [ "$have_meson" != "$v_meson" ]; then
	echo "Warning: meson on PATH is '$have_meson', expected $v_meson (v_meson in include/depinfo.sh)" >&2
	[ $IN_CI -eq 1 ] && exit 1
fi
exit 0
