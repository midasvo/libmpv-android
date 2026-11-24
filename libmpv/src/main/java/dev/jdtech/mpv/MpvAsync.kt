@file:JvmName("MpvAsync")

package dev.jdtech.mpv

import dev.jdtech.mpv.MPVLib.MpvError
import dev.jdtech.mpv.MPVLib.MpvFormat
import dev.jdtech.mpv.internal.Native
import dev.jdtech.mpv.internal.NodeCodec

/*
 * Asynchronous calls on mpv's own async API. Each sends a request tagged with a fresh
 * reply_userdata and suspends until mpv's reply event; the calling thread is never
 * blocked, so these are safe on the main thread. They resume in the caller's coroutine
 * context.
 *
 * Common to all three:
 * - mpv's error is thrown as MpvException (also when the request cannot be queued);
 * - called after MPVLib.destroy() they throw IllegalStateException, and when the instance
 *   is destroyed while one is pending, it throws IllegalStateException as destroy()
 *   returns (mpv's reply can no longer come);
 * - cancelling the coroutine returns at once with CancellationException; what mpv does
 *   with the request is described per function.
 * Requests sent one after another from one coroutine reach mpv in that order.
 */

/**
 * Runs a command asynchronously and returns its result ([MpvNode.None] if it has none).
 * [args] is as for [commandNode]. Maps to `mpv_command_node_async`.
 *
 * Cancellation calls `mpv_abort_async_command`, which stops commands that support it
 * (such as `subprocess`) early; others run to completion and their result is discarded.
 *
 * @throws MpvException with mpv's error, e.g. `MPV_ERROR_COMMAND`
 * @throws IllegalStateException if the instance is or gets destroyed
 */
suspend fun MPVLib.commandAsync(args: MpvNode): MpvNode {
    val encoded = NodeCodec.encode(args)
    return runtime.request("commandAsync", abortable = true) { handle, id ->
        Native.commandNodeAsync(handle, id, encoded)
    }
}

/**
 * Reads property [name] asynchronously, as [getPropertyNode] does. Maps to
 * `mpv_get_property_async(MPV_FORMAT_NODE)`.
 *
 * Cancellation only stops waiting: mpv cannot abort a property read.
 *
 * @throws MpvException with mpv's error, e.g. `MPV_ERROR_PROPERTY_UNAVAILABLE`
 * @throws IllegalStateException if the instance is or gets destroyed
 */
suspend fun MPVLib.getPropertyAsync(name: String): MpvNode =
    propertyAsync("getPropertyAsync", name, MpvFormat.MPV_FORMAT_NODE)

/**
 * Reads the flag property [name] (`pause`, `mute`, `eof-reached`, ...) asynchronously,
 * without unwrapping an [MpvNode]. Maps to `mpv_get_property_async(MPV_FORMAT_FLAG)`: mpv
 * converts the value as for [booleanPropertyFlow]. Otherwise as [getPropertyAsync].
 *
 * ```
 * val paused = mpv.getBooleanPropertyAsync("pause")
 * ```
 *
 * @throws MpvException with mpv's error, e.g. `MPV_ERROR_PROPERTY_UNAVAILABLE`, or
 *   `MPV_ERROR_PROPERTY_FORMAT` if the property is not a flag
 * @throws IllegalStateException if the instance is or gets destroyed
 */
suspend fun MPVLib.getBooleanPropertyAsync(name: String): Boolean =
    typedPropertyAsync("getBooleanPropertyAsync", name, MpvFormat.MPV_FORMAT_FLAG) { it.boolean }

/**
 * Reads the integer property [name] (`playlist-pos`, `playlist-count`, ...)
 * asynchronously. Maps to `mpv_get_property_async(MPV_FORMAT_INT64)`: mpv converts the
 * value as for [longPropertyFlow], so a time comes back as whole seconds (use
 * [getDoublePropertyAsync]). Otherwise as [getPropertyAsync].
 *
 * @throws MpvException with mpv's error, e.g. `MPV_ERROR_PROPERTY_UNAVAILABLE`, or
 *   `MPV_ERROR_PROPERTY_FORMAT` if the property is not a number
 * @throws IllegalStateException if the instance is or gets destroyed
 */
suspend fun MPVLib.getLongPropertyAsync(name: String): Long =
    typedPropertyAsync("getLongPropertyAsync", name, MpvFormat.MPV_FORMAT_INT64) { it.long }

/**
 * Reads the numeric property [name] (`time-pos`, `duration`, `volume`, ...)
 * asynchronously. Maps to `mpv_get_property_async(MPV_FORMAT_DOUBLE)`: mpv converts the
 * value as for [doublePropertyFlow]. Otherwise as [getPropertyAsync].
 *
 * ```
 * val position = mpv.getDoublePropertyAsync("time-pos")
 * ```
 *
 * @throws MpvException with mpv's error, e.g. `MPV_ERROR_PROPERTY_UNAVAILABLE` (`duration`
 *   while nothing is loaded), or `MPV_ERROR_PROPERTY_FORMAT` if the property is not a
 *   number
 * @throws IllegalStateException if the instance is or gets destroyed
 */
suspend fun MPVLib.getDoublePropertyAsync(name: String): Double =
    typedPropertyAsync("getDoublePropertyAsync", name, MpvFormat.MPV_FORMAT_DOUBLE) { it.double }

/**
 * Reads property [name] formatted as a string (any property; `media-title`, `path`, ...)
 * asynchronously. Maps to `mpv_get_property_async(MPV_FORMAT_STRING)`: the same text as
 * [stringPropertyFlow] and [MPVLib.getPropertyString] (a structured property comes back
 * as JSON; use [getPropertyAsync] for it). Otherwise as [getPropertyAsync].
 *
 * @throws MpvException with mpv's error, e.g. `MPV_ERROR_PROPERTY_UNAVAILABLE`
 * @throws IllegalStateException if the instance is or gets destroyed
 */
suspend fun MPVLib.getStringPropertyAsync(name: String): String =
    typedPropertyAsync("getStringPropertyAsync", name, MpvFormat.MPV_FORMAT_STRING) { it.string }

/** One mpv_get_property_async in [format]; the value comes back as the matching node. */
private suspend fun MPVLib.propertyAsync(operation: String, name: String, format: Int): MpvNode =
    runtime.request("$operation($name)", abortable = false) { handle, id ->
        Native.getPropertyAsync(handle, id, name, format)
    }

/**
 * [propertyAsync] for a scalar [format], unwrapped by [value]. mpv answers a successful
 * read in the format asked for, so a node of another kind does not happen; it would be
 * reported as `MPV_ERROR_PROPERTY_FORMAT`.
 */
private suspend inline fun <T : Any> MPVLib.typedPropertyAsync(
    operation: String,
    name: String,
    format: Int,
    value: (MpvNode) -> T?,
): T {
    val node = propertyAsync(operation, name, format)
    return value(node) ?: throw MpvException(MpvError.MPV_ERROR_PROPERTY_FORMAT, "$operation($name)")
}

/**
 * Sets property [name] to [value] asynchronously, as [setPropertyNode] does. Maps to
 * `mpv_set_property_async(MPV_FORMAT_NODE)`.
 *
 * Cancellation only stops waiting: the property may still be set.
 *
 * @throws MpvException with mpv's error, e.g. `MPV_ERROR_PROPERTY_FORMAT`
 * @throws IllegalStateException if the instance is or gets destroyed
 */
suspend fun MPVLib.setPropertyAsync(name: String, value: MpvNode) {
    val encoded = NodeCodec.encode(value)
    runtime.request("setPropertyAsync($name)", abortable = false) { handle, id ->
        Native.setPropertyAsync(handle, id, name, encoded)
    }
}
