@file:JvmName("MpvFlows")

package dev.jdtech.mpv

import dev.jdtech.mpv.MPVLib.MpvFormat
import dev.jdtech.mpv.internal.NodeCodec
import kotlinx.coroutines.flow.Flow

/*
 * Property changes and events as Kotlin flows.
 *
 * All of them are cold: nothing is observed until a collector starts, and each collector
 * has its own observation. It begins synchronously when collection starts, before the
 * collector's first suspension (so `launch(start = CoroutineStart.UNDISPATCHED) { ... }`
 * has it in place when launch returns), and ends when the collection does, for whatever
 * reason: property flows then call mpv_unobserve_property. When the instance is destroyed
 * the flows complete normally; collecting one after destroy() throws
 * IllegalStateException.
 *
 * mpv's event thread never waits for a collector:
 * - property flows are conflated: a collector that is slower than the changes gets the
 *   latest value and skips the ones in between (mpv itself coalesces property changes the
 *   same way);
 * - the event flow buffers 256 events per collector; beyond that the newest are dropped
 *   and replaced by one MpvPlayerEvent.Dropped with their number.
 * Values are delivered in the collector's coroutine context; use flowOn/collect on the
 * dispatcher you need.
 */

/**
 * Changes of property [name] as structured values ([MpvNode.None] while the property is
 * unavailable). Starts with the current value, as every mpv observation does. Maps to
 * `mpv_observe_property(MPV_FORMAT_NODE)` and `mpv_unobserve_property`.
 *
 * ```
 * mpv.nodePropertyFlow("track-list").collect { tracks ->
 *     val audio = tracks.list.orEmpty().filter { it["type"]?.string == "audio" }
 * }
 * ```
 *
 * See the file's notes above for when observation starts and stops, conflation, and
 * destroy().
 */
fun MPVLib.nodePropertyFlow(name: String): Flow<MpvNode> =
    runtime.propertyFlow(name, MpvFormat.MPV_FORMAT_NODE) { value ->
        (value as? ByteArray)?.let(NodeCodec::decode) ?: MpvNode.None
    }

/**
 * Changes of the flag property [name] (`pause`, `eof-reached`, `paused-for-cache`, ...);
 * null while it is unavailable. Maps to `mpv_observe_property(MPV_FORMAT_FLAG)`; see
 * [nodePropertyFlow] for the rest.
 */
fun MPVLib.booleanPropertyFlow(name: String): Flow<Boolean?> =
    runtime.propertyFlow(name, MpvFormat.MPV_FORMAT_FLAG) { it as? Boolean }

/**
 * Changes of the integer property [name] (`playlist-pos`, `playlist-count`, ...); null
 * while it is unavailable. Maps to `mpv_observe_property(MPV_FORMAT_INT64)`; note that
 * mpv rounds a time observed this way to whole seconds (use [doublePropertyFlow]). See
 * [nodePropertyFlow] for the rest.
 */
fun MPVLib.longPropertyFlow(name: String): Flow<Long?> =
    runtime.propertyFlow(name, MpvFormat.MPV_FORMAT_INT64) { it as? Long }

/**
 * Changes of the numeric property [name] (`time-pos`, `duration`, `speed`, ...); null
 * while it is unavailable. Maps to `mpv_observe_property(MPV_FORMAT_DOUBLE)`; see
 * [nodePropertyFlow] for the rest.
 */
fun MPVLib.doublePropertyFlow(name: String): Flow<Double?> =
    runtime.propertyFlow(name, MpvFormat.MPV_FORMAT_DOUBLE) { it as? Double }

/**
 * Changes of property [name] formatted as a string (any property; `media-title`,
 * `sub-text`, ...); null while it is unavailable. Maps to
 * `mpv_observe_property(MPV_FORMAT_STRING)`; see [nodePropertyFlow] for the rest.
 */
fun MPVLib.stringPropertyFlow(name: String): Flow<String?> =
    runtime.propertyFlow(name, MpvFormat.MPV_FORMAT_STRING) { it as? String }

/**
 * mpv's events with their data (see [MpvPlayerEvent]), in the order mpv sent them. Each
 * collector gets every event from the moment it starts collecting; the
 * [MPVLib.EventObserver]s keep getting them too. Built on the events libplayer's event
 * thread receives from `mpv_wait_event`.
 *
 * ```
 * mpv.eventFlow().filterIsInstance<MpvPlayerEvent.EndFile>().collect { end ->
 *     if (end.reason == MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_ERROR) showError(end.error)
 * }
 * ```
 *
 * See the file's notes above for buffering (256 events, then [MpvPlayerEvent.Dropped]),
 * when collection starts, and destroy().
 */
fun MPVLib.eventFlow(): Flow<MpvPlayerEvent> = runtime.eventFlow()
