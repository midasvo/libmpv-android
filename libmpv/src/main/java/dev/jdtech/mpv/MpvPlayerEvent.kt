package dev.jdtech.mpv

/**
 * An mpv event, with the data mpv sends with it, as [eventFlow] delivers it. Property
 * changes are not events here (collect a property flow instead), nor are log messages
 * (see [MPVLib.LogObserver]) or the replies to asynchronous calls (the suspend functions
 * return them).
 */
sealed interface MpvPlayerEvent {
    /** `MPV_EVENT_START_FILE`: mpv starts loading the playlist entry [playlistEntryId]. */
    data class StartFile(val playlistEntryId: Long) : MpvPlayerEvent

    /** `MPV_EVENT_FILE_LOADED`: the file is loaded and playback (or the pause) begins. */
    data object FileLoaded : MpvPlayerEvent

    /**
     * `MPV_EVENT_END_FILE`: playback of [playlistEntryId] ended, for [reason] (one of
     * [MPVLib.MpvEndFileReason]). For `MPV_END_FILE_REASON_ERROR`, [error] is the
     * [MPVLib.MpvError] code; otherwise it is 0. For `MPV_END_FILE_REASON_REDIRECT`
     * (a playlist file was expanded), [playlistInsertId] is the id of the first inserted
     * entry and [playlistInsertNumEntries] their number; otherwise both are 0.
     */
    data class EndFile(
        val reason: Int,
        val error: Int,
        val playlistEntryId: Long,
        val playlistInsertId: Long,
        val playlistInsertNumEntries: Int,
    ) : MpvPlayerEvent

    /** `MPV_EVENT_CLIENT_MESSAGE`: a `script-message` with its arguments. */
    data class ClientMessage(val args: List<String>) : MpvPlayerEvent

    /** `MPV_EVENT_SEEK`: a seek started. */
    data object Seek : MpvPlayerEvent

    /** `MPV_EVENT_PLAYBACK_RESTART`: playback (re)started after loading or seeking. */
    data object PlaybackRestart : MpvPlayerEvent

    /** `MPV_EVENT_VIDEO_RECONFIG`: the video output's parameters may have changed. */
    data object VideoReconfig : MpvPlayerEvent

    /** `MPV_EVENT_AUDIO_RECONFIG`: the audio output's parameters may have changed. */
    data object AudioReconfig : MpvPlayerEvent

    /** `MPV_EVENT_SHUTDOWN`: mpv is quitting (the `quit` command); destroy the instance. */
    data object Shutdown : MpvPlayerEvent

    /** `MPV_EVENT_QUEUE_OVERFLOW`: mpv's own event queue overflowed and mpv dropped events. */
    data object QueueOverflow : MpvPlayerEvent

    /**
     * This collector fell more than the flow's buffer behind (see [eventFlow]) and [count]
     * events were dropped here, at this point of the sequence.
     */
    data class Dropped(val count: Int) : MpvPlayerEvent

    /** Any other event, by its [MPVLib.MpvEvent] id (`MPV_EVENT_IDLE`, and ids newer mpv adds). */
    data class Other(val eventId: Int) : MpvPlayerEvent
}
