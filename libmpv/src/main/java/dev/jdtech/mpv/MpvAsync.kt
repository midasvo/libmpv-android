@file:JvmName("MpvAsync")

package dev.jdtech.mpv

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
    runtime.request("getPropertyAsync($name)", abortable = false) { handle, id ->
        Native.getPropertyAsync(handle, id, name)
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
