# ADR 0005: Add the new API on top of `MPVLib`, without changing it

- **Status:** accepted
- **Date:** 2026-11-24
- **Context links:** #49 (the change), ADR 0001, ADR 0003

## Context

`MPVLib` offers what upstream's wrapper offers: typed getters and setters for scalars, string
commands, and observer callbacks on mpv's event thread. A player built on it, such as Ephyra's,
works around the gaps:

- **Structured properties** (`track-list`, `chapter-list`, `metadata`) only come out as JSON text
  through `getPropertyString`. The player parses it by hand.
- **Every call blocks** until mpv's core answers, which can take long while mpv opens a file. On the
  main thread that is a stutter or an ANR. mpv has an async API for this that `MPVLib` does not use.
- **Changes and events arrive as callbacks** with a property name or an event id, and the data mpv
  sends with an event (end-file reason, playlist ids, script message arguments) is mostly lost.
- **mpv can only read what FFmpeg and mpv can open.** An app with its own HTTP client, or a
  `content://` URI, has no way in.

Apps compiled against 1.3.0 must keep working, and the Kotlin package stays `dev.jdtech.mpv`
(ADR 0001).

## Decision

Add the new API as extension functions on `MPVLib` in new files (`MpvNodes.kt`, `MpvAsync.kt`,
`MpvFlows.kt`, `MpvStreams.kt`) with new types (`MpvNode`, `MpvPlayerEvent`, `MpvStream`,
`MpvException`). Nothing in the 1.3.0 API changes: its methods keep returning codes or `null`, and
the `EventObserver` and `LogObserver` callbacks get exactly what they got before. Only the new
functions throw `MpvException`.

How it is built (details in the module docs of `node.rs`, `observations.rs`, `stream.rs` and
`internal/MpvRuntime.kt`):

- **A node crosses JNI as one `byte[]`**, in a small tagged format that `node.rs` writes and
  `NodeCodec.kt` reads (and the other way round). One JNI call per value instead of one per node, no
  local references to manage on the event thread, and both sides tested without a VM against the
  same bytes. Strings in it are UTF-16, converted like every other string (ADR 0003).
- **The event thread routes by `reply_userdata`.** Async requests and property flows get positive
  ids from one Kotlin counter, never reused; `observeProperty` gets negative ones from libplayer.
  Only negative ids reach the `EventObserver` callbacks, so replies and flow changes never show up
  there.
- **mpv's event thread never waits for Kotlin.** A reply completes a deferred; a property change
  goes to a conflated channel (a slow collector sees the latest value); an event goes to a queue of
  256 per collector, after which the newest are dropped and the collector gets one
  `MpvPlayerEvent.Dropped(count)`. One slow collector cannot stall playback or the other
  collectors.
- **Async calls use mpv's async API** (`mpv_command_node_async` and the property equivalents), not a
  thread pool around the blocking calls. No thread is blocked, a cancelled command reaches
  `mpv_abort_async_command`, and `destroy()` fails what is still pending with
  `IllegalStateException` instead of leaving it suspended forever.
- **A stream is a registry id, not a pointer.** mpv can call a stream's `cancel_fn` after its
  `close_fn`, and a protocol's `open_fn` while the instance is being destroyed. The ids are never
  reused, so a late call finds nothing instead of freed memory. A stream callback that calls back
  into the same instance (which deadlocks mpv) throws `IllegalStateException`.
- **`kotlinx-coroutines-core` is an `api` dependency**, because `suspend` functions and `Flow` are in
  the public signatures.
- **The public API is a checked-in contract:** `libmpv/api/libmpv.api`, compared with the built AAR
  by `buildscripts/check-api.sh` in CI (RELEASING.md, "The API contract").

## Consequences

- Apps built against 1.3.0 link unchanged; the API dump of 1.3.0's surface only gained lines.
- There are now two ways to do most things: the classic calls and the new ones. The classic ones
  stay for compatibility and for code that wants return codes; the README says when to prefer which.
- Every app that uses the library gets `kotlinx-coroutines-core` on its classpath. Gradle resolves
  it to the highest version any dependency asks for, so an app can be on a newer one. It is a JVM
  dependency and is not in the SBOM, which lists the native components.
- The event flow can lose events for a collector that falls 256 events behind. It is told so
  (`Dropped`), and property state is never lost that way: property flows are conflated, not
  dropped.
- Upstream has none of this. `MPVLib.kt` changed a little to hold the per-instance runtime, so an
  upstream change to it needs care in the sync (RELEASING.md).
- A removed or changed line in the API dump is a breaking change and needs a major version.

## Alternatives considered

- **Change `MPVLib`'s methods** (throw instead of returning codes, return nodes): a cleaner single
  API, but it breaks every app compiled against 1.3.0. Rejected; possible in a major release.
- **Keep JSON strings for structured values.** No new types, but every app parses JSON on the hot
  path and loses mpv's types (an integer and a double look the same).
- **Build a tree of Java objects in native code.** One JNI call and one local reference per node,
  managed by hand on the event thread; the byte format is simpler and faster.
- **Deliver async replies and flow changes through `EventObserver`.** No routing, but observers would
  see events they never asked for, which changes what 1.3.0 code receives.
- **An unbounded event buffer, or one that suspends the event thread when full.** The first can grow
  without limit behind a stuck collector; the second lets one collector stall playback for everyone.

## Revisit when

- A major release is planned: decide whether the classic calls should go, or `MPVLib` should
  become the new API directly (together with ADR 0001's package question).
- Apps report lost events in practice: make the event buffer size a parameter of `eventFlow()`.
