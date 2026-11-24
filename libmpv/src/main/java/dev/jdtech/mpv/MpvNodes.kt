@file:JvmName("MpvNodes")

package dev.jdtech.mpv

import dev.jdtech.mpv.internal.Native
import dev.jdtech.mpv.internal.NodeCodec

/*
 * Structured (mpv_node) properties and commands, synchronous. Like the other MPVLib
 * methods these block the calling thread until mpv's core has answered, which is usually
 * quick but can take long while mpv is busy (opening a file, for example): prefer the
 * suspend functions in MpvAsync.kt on the main thread.
 */

/**
 * Reads property [name] as a structured value: maps and arrays such as `track-list`,
 * `chapter-list` or `metadata` come back as [MpvNode.MapValue]/[MpvNode.ArrayValue]
 * trees, scalars as the matching node. Maps to `mpv_get_property(MPV_FORMAT_NODE)`.
 *
 * Blocks the calling thread; may be called from any thread, including the observer
 * callbacks.
 *
 * @throws MpvException with mpv's error, e.g. `MPV_ERROR_PROPERTY_UNAVAILABLE`
 * @throws IllegalStateException after [MPVLib.destroy]
 */
fun MPVLib.getPropertyNode(name: String): MpvNode =
    NodeCodec.decode(Native.getPropertyNode(handle(), name))

/**
 * Sets property [name] to a structured [value]; mpv converts it to the property's type
 * where it can. Maps to `mpv_set_property(MPV_FORMAT_NODE)`.
 *
 * Blocks the calling thread; may be called from any thread.
 *
 * @throws MpvException with mpv's error, e.g. `MPV_ERROR_PROPERTY_FORMAT`
 * @throws IllegalStateException after [MPVLib.destroy]
 * @throws IllegalArgumentException if [value] is nested deeper than 64 levels
 */
fun MPVLib.setPropertyNode(name: String, value: MpvNode) {
    Native.setPropertyNode(handle(), name, NodeCodec.encode(value))
}

/**
 * Runs a command and returns its result ([MpvNode.None] for the many commands that have
 * none). [args] is an [MpvNode.ArrayValue] of positional arguments (build it with
 * [MpvNode.command]) or an [MpvNode.MapValue] of named ones ([MpvNode.namedCommand]).
 * Unlike [MPVLib.command], arguments keep their types, no string expansion or OSD is
 * applied, and failures are reported. Maps to `mpv_command_node`.
 *
 * ```
 * val r = mpv.commandNode(MpvNode.command("loadfile", url, "append"))
 * val entryId = r["playlist_entry_id"]?.long
 * ```
 *
 * Blocks the calling thread until mpv has run the command (for `loadfile`, that is until
 * the file is queued, not loaded); may be called from any thread.
 *
 * @throws MpvException with mpv's error, e.g. `MPV_ERROR_COMMAND`
 * @throws IllegalStateException after [MPVLib.destroy]
 */
fun MPVLib.commandNode(args: MpvNode): MpvNode =
    NodeCodec.decode(Native.commandNode(handle(), NodeCodec.encode(args)))

/**
 * Undoes [MPVLib.observeProperty]: every observation of [property] made with it stops,
 * so the [MPVLib.EventObserver]s get no more changes of it (one already on its way may
 * still arrive). Property flows of the same property are not affected. Maps to
 * `mpv_unobserve_property`.
 *
 * May be called from any thread.
 *
 * @return the number of observations removed (0 if the property was not observed), or a
 *   negative [MPVLib.MpvError] code
 * @throws IllegalStateException after [MPVLib.destroy]
 */
fun MPVLib.unobserveProperty(property: String): Int =
    Native.unobserveLegacyProperty(handle(), property)
