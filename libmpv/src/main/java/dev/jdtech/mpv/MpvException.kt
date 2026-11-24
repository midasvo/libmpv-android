package dev.jdtech.mpv

import dev.jdtech.mpv.internal.describeMpvError

/**
 * An mpv call failed with mpv's error code [error] (one of [MPVLib.MpvError], always
 * negative), for example `MPV_ERROR_PROPERTY_UNAVAILABLE` when reading `duration` while
 * nothing is loaded. Thrown by the node, async and stream functions of this library; the
 * older [MPVLib] methods keep returning codes or null as before.
 *
 * A call on a destroyed instance throws [IllegalStateException] instead, as every
 * [MPVLib] method does.
 */
class MpvException(
    /** mpv's error code, as `mpv_error` (see [MPVLib.MpvError]). */
    val error: Int,
    operation: String,
) : RuntimeException("$operation failed: ${describeMpvError(error)} ($error)")
