# Architecture decision records

Why the fork is built the way it is. Each record says what was decided, what it costs, and when to
look at it again. A record is not rewritten when things change: it gets a dated update, or a new
record replaces it.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-keep-the-kotlin-package.md) | Keep the Kotlin package `dev.jdtech.mpv`; the fork's identity is its Maven coordinate | accepted (updated 2026-11-24: to decide again before a major release) |
| [0002](0002-no-vulkan-for-now.md) | Keep Vulkan disabled: on Android it would lose zero-copy MediaCodec decoding | accepted |
| [0003](0003-rust-jni-wrapper.md) | Write the JNI wrapper in Rust, with handles as table keys and a gate around `destroy()` | accepted |
| [0004](0004-build-pipeline.md) | One build definition, one CI job per ABI with a cache, Gradle only packages | accepted |
| [0005](0005-api-on-top-of-mpvlib.md) | Add the new API as extension functions on `MPVLib`, without changing it | accepted |

New records take the next number and the same headings: Status, Date, Context links; Context,
Decision, Consequences, Alternatives considered, Revisit when.
