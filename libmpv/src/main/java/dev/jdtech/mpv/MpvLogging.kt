@file:JvmName("MpvLogging")

package dev.jdtech.mpv

import dev.jdtech.mpv.internal.Native

/**
 * Sets the most verbose of mpv's log messages that libplayer also writes to logcat (tag
 * `mpv`), as one of [MPVLib.MpvLogLevel]: `MPV_LOG_LEVEL_WARN` keeps errors and warnings
 * there, `MPV_LOG_LEVEL_NONE` stops the echo. The default is `MPV_LOG_LEVEL_TRACE`: every
 * message mpv sends is echoed, as before this function existed.
 *
 * Only the logcat echo changes: the [MPVLib.LogObserver]s keep getting every message at the
 * level set with [MPVLib.requestLogMessages] (default "v"), which also bounds what can be
 * echoed. Verbose messages include the URLs mpv opens, so an app that plays authenticated
 * URLs and still wants mpv's log for itself can keep it out of logcat this way:
 *
 * ```
 * mpv.setLogcatLevel(MPVLib.MpvLogLevel.MPV_LOG_LEVEL_WARN)
 * mpv.addLogObserver(object : MPVLib.LogObserver {
 *     override fun logMessage(prefix: String, level: Int, text: String) = appLog.add(prefix, level, text)
 * })
 * ```
 *
 * Takes effect for the messages the event thread delivers from then on. libplayer's own
 * messages under the same tag (errors of the classic calls, and one line per event) are not
 * affected. May be called from any thread, before or after [MPVLib.init].
 *
 * @throws IllegalStateException after [MPVLib.destroy]
 */
fun MPVLib.setLogcatLevel(level: Int) {
    Native.setLogcatLevel(handle(), level)
}
