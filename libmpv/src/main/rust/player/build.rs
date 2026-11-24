//! Links libplayer.so for Android: against the libmpv.so and libavcodec.so in
//! `LIBMPV_LIB_DIR` (one ABI's directory) and the NDK's liblog, with 16 KiB
//! pages. Nothing is linked for other targets, so `cargo check` and the host
//! unit tests need neither the variable nor the libraries.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LIBMPV_LIB_DIR");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }

    match env::var("LIBMPV_LIB_DIR") {
        Ok(dir) if !dir.is_empty() => println!("cargo:rustc-link-search=native={dir}"),
        _ => println!(
            "cargo:warning=LIBMPV_LIB_DIR is not set: linking will fail (cargo check is fine). \
             Set it to the directory with this ABI's libmpv.so and libavcodec.so."
        ),
    }
    println!("cargo:rustc-link-lib=dylib=mpv");
    println!("cargo:rustc-link-lib=dylib=avcodec");
    println!("cargo:rustc-link-lib=dylib=log");

    // Android 15+ devices may use 16 KiB pages: align the LOAD segments for
    // them (harmless with 4 KiB pages). The NDK's default is 4 KiB on
    // 32-bit ABIs and depends on the NDK version on 64-bit ones.
    println!("cargo:rustc-cdylib-link-arg=-Wl,-z,max-page-size=16384");
}
