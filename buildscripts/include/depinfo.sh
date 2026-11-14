#!/bin/bash -e

## Dependency versions

v_platform=android-37.0
v_sdk=14742923_latest
v_ndk=30.0.16248370
v_sdk_build_tools=37.0.0
v_cmake=4.1.2

v_lua=5.2.4
v_libunibreak=8_0
v_libass=0.17.5
v_harfbuzz=15.1.0
v_fribidi=1.0.18
v_freetype=2-15-0
v_libxml2=2.15.4
v_fontconfig=2.18.3
v_mbedtls=3.6.7
v_libplacebo=7.387.0
v_dav1d=1.5.4
v_ffmpeg=9.0.2
v_curl=8_23_0
v_mpv=0.42.0

# Pinned commit of FFmpeg/gas-preprocessor (used by download-sdk.sh)
v_gas_preprocessor=ac1836309c2e77023c228b7184485597286289d3

## SHA-256 of the release tarballs (verified by download-deps.sh).
## Update by hand together with the matching version above.

sha256_lua=b9e2e4aad6789b3b63a056d442f7b39f0ecfca3ae0f1fc0ae4e9614401b69f4b
sha256_libunibreak=9c4fad6e517338a098373acc9f35579ae2c325e6446666fb9ac2666ba15ceba4
sha256_curl=e3de77cfa6367083c776a26647fc9f633b538c06c3baa73d931a252ac8f72b49


## Dependency tree
# I would've used a dict but putting arrays in a dict is not a thing

dep_mbedtls=()
dep_curl=(mbedtls)
dep_dav1d=()
dep_ffmpeg=(mbedtls dav1d libxml2)
dep_freetype2=()
dep_fontconfig=(libxml2 freetype)
dep_fribidi=()
dep_harfbuzz=()
dep_libunibreak=()
dep_libass=(freetype fontconfig fribidi harfbuzz libunibreak)
dep_lua=()
dep_libplacebo=()
dep_mpv=(ffmpeg libass lua libplacebo curl)
dep_mpv_android=(mpv)
