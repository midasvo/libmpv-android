# ADR 0003: Write the JNI wrapper in Rust

- **Status:** accepted
- **Date:** 2026-11-24
- **Context links:** #49 (the change), #47 (the last fixes to the C++ wrapper), ADR 0001

## Context

`libplayer.so` is the JNI glue between `dev.jdtech.mpv.MPVLib` and libmpv. Up to and including
1.3.0 it was the C++ wrapper inherited from upstream: about 860 lines in `libmpv/src/main/cpp`,
built by Gradle through CMake.

Its design had one problem that no local fix removed: Kotlin held a raw pointer. `nativeCreate`
returned `reinterpret_cast<jlong>(new MPVInstance())`, every other function cast the `Long` back,
and `nativeDestroy` ended with `delete`. Nothing synchronised the calls. That gave these defects:

- **Use after free.** A call on one thread racing `destroy()` on another used the instance while it
  was being freed. `MPVLib.kt` cleared its `nativeInstance` only after `nativeDestroy` returned, and
  the field was not volatile. `nativeAttachSurface` and `nativeDetachSurface` did not even check
  that mpv still existed.
- **Double free.** Two concurrent `destroy()` calls could both pass the check in `MPVLib.kt` and
  both `delete`.

Before that, the fork had already fixed a series of smaller defects in the same files: exceptions
thrown by observers, invalid UTF-8 from mpv passed to `NewStringUTF` (JNI's Modified UTF-8 is
undefined for it), leaked local references, `destroy()` from a callback joining the event thread
from itself, and unchecked null and out-of-memory results for string arguments (#47 and earlier).
All of them are the same kind of bug: a lifetime or an error path managed by hand across the JNI
boundary.

The wrapper is also about to grow: structured `mpv_node` values, mpv's async API and custom stream
protocols all add callbacks and memory that crosses the boundary (they have since been added:
[ADR 0005](0005-api-on-top-of-mpvlib.md)).

## Decision

Replace the C++ wrapper with a Rust crate, `player` in `libmpv/src/main/rust`, built into
`libplayer.so`. `MPVLib.kt` stays as it is: the `native*` signatures and the exported
`Java_dev_jdtech_mpv_MPVLib_*` names are the same, so apps built against 1.3.0 keep working.

The ownership design (module docs in `player/src/registry.rs`, `instance.rs` and `lifecycle.rs`):

- **A handle is a key, not a pointer.** The registry maps handles to instances. Handles count up
  from 1 and are never reused. A stale, unknown or forged handle finds nothing and the call throws
  `IllegalStateException`. `nativeDestroy` on an unknown handle does nothing, like a second
  `destroy()`.
- **The registry owns each instance** (an `Arc`) from `create` until `destroy`. Every call takes
  its own `Arc` for its duration, so the memory outlives any call that found it.
- **A gate decides whether mpv may still be used.** It counts the calls in flight and closes in two
  steps: `destroy()` first joins the event thread while calls are still admitted (an observer
  callback may call back into the instance), then refuses new calls and waits for the ones in
  flight before it calls `mpv_terminate_destroy`. No call ever blocks to enter, so a callback cannot
  deadlock against `destroy()`. The one way to wedge it is a Kotlin callback that blocks on the
  thread calling `destroy()`, which `MPVLib`'s documentation forbids.
- **Panics never cross JNI.** Each entry point catches them and throws `RuntimeException` instead.
  The release profile keeps `panic = "unwind"` for this.
- **Cleanup is `Drop`.** Global references, mpv strings and the thread attachment are released by
  destructors, on every path.
- **Strings go through UTF-16** (`NewString`, `GetStringRegion`), converted in `text.rs`, so
  Modified UTF-8 is never involved.

The only dependency is `jni-sys`. The mpv, libavcodec and liblog functions are declared by hand in
`ffi.rs`, checked against the headers of mpv 0.42.0 (`client.h`, client API 2.6, and
`stream_cb.h`), so the build needs no bindgen and no libclang.

## Consequences

- The defects above are gone by construction, not by care in each function. The modules that need
  neither JNI nor mpv (the lifecycle, the registry, the text conversion, the event and node
  decoding, the stream bookkeeping) are unit tested on the host with `cargo test` in
  `libmpv/src/main/rust`. CI runs those tests, `cargo fmt --check` and `cargo clippy` in the
  Assemble job of `build.yaml`. The rest is covered by the instrumented tests in
  `libmpv/src/androidTest` on an x86_64 emulator.
- The build needs a Rust toolchain: `rustup`, with the version and the four Android targets pinned
  in `rust-toolchain.toml`. `scripts/player.sh` points cargo at the NDK's clang, so no `cargo-ndk`.
  Gradle no longer runs CMake or needs the NDK (see ADR 0004).
- The corresponding source of a release includes the crates from `Cargo.lock`, vendored under
  `libmpv/src/main/rust/vendor`.
- The declarations in `ffi.rs` are not checked by a compiler against the headers. A change to mpv's
  client API that the wrapper uses (a struct layout, an enum value) has to be found by reading the
  mpv release notes and `client.h`. A Renovate bump of mpv is the moment to do that.
- Upstream changes to its C++ wrapper no longer merge. Each one has to be read and ported to the
  crate by hand (RELEASING.md, "Keeping up with upstream").
- Contributors to the wrapper need Rust. The Kotlin side is unchanged.
- The crate still contains `unsafe` code, in `ffi.rs`, `jni.rs` and wherever a raw mpv or JNI value
  is turned into a safe type. The rules for it are in the module docs.

## Alternatives considered

- **Fix the C++ wrapper in place:** a handle table, a mutex and a call counter in C++. Possible, and
  less to build. Rejected: it adds the same bookkeeping in a language where every new function has
  to repeat the error and cleanup handling by hand, and the next features (async calls, node values,
  stream callbacks) add more of exactly that.
- **Guard it from Kotlin:** a read-write lock in `MPVLib.kt` around every native call. Rejected:
  `destroy()` would take the write lock while an observer callback, holding the read lock, calls
  back into the instance. Either callbacks may not call `MPVLib` (they can today) or it deadlocks.
  It also does not help a stale handle passed to native code.
- **Rust with the `jni` crate and bindgen.** Less code by hand, but more dependencies in every
  release's source and a libclang at build time. The wrapper uses few functions; declaring them is
  cheaper than the toolchain.

## Revisit when

- mpv changes its client API in a way `ffi.rs` uses: update the declarations (or generate them, if
  that has become the cheaper option).
- Upstream rewrites its wrapper or changes the JNI interface of `MPVLib`: decide whether to port
  the change or to keep the interfaces apart for good.
