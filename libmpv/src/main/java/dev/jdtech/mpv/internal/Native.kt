package dev.jdtech.mpv.internal

/**
 * The native side of the node, async, observation and stream API (libplayer's `api.rs`).
 * Nodes are passed encoded by [NodeCodec]. An unknown or destroyed handle throws
 * [IllegalStateException]; an mpv error throws [dev.jdtech.mpv.MpvException]. The library
 * is loaded by [dev.jdtech.mpv.MPVLib], which every handle comes from.
 */
internal object Native {
    /** mpv_get_property(MPV_FORMAT_NODE) */
    @JvmStatic external fun getPropertyNode(handle: Long, name: String): ByteArray

    /** mpv_set_property(MPV_FORMAT_NODE) */
    @JvmStatic external fun setPropertyNode(handle: Long, name: String, value: ByteArray)

    /** mpv_command_node */
    @JvmStatic external fun commandNode(handle: Long, args: ByteArray): ByteArray

    /** mpv_command_node_async: the reply comes to MPVLib.onReply with [id] */
    @JvmStatic external fun commandNodeAsync(handle: Long, id: Long, args: ByteArray)

    /** mpv_abort_async_command */
    @JvmStatic external fun abortAsyncCommand(handle: Long, id: Long)

    /** mpv_get_property_async(MPV_FORMAT_NODE) */
    @JvmStatic external fun getPropertyAsync(handle: Long, id: Long, name: String)

    /** mpv_set_property_async(MPV_FORMAT_NODE) */
    @JvmStatic external fun setPropertyAsync(handle: Long, id: Long, name: String, value: ByteArray)

    /**
     * mpv_observe_property with reply_userdata [id]: changes come to MPVLib.onPropertyChange
     * for positive ids, to the EventObserver callbacks for negative ones.
     */
    @JvmStatic external fun observeProperty(handle: Long, id: Long, name: String, format: Int)

    /** mpv_unobserve_property: the number of observations removed, or a negative mpv error */
    @JvmStatic external fun unobserveProperty(handle: Long, id: Long): Int

    /**
     * mpv_unobserve_property for every observation of [name] made with MPVLib.observeProperty
     * (whose ids libplayer keeps): the number removed, or a negative mpv error.
     */
    @JvmStatic external fun unobserveLegacyProperty(handle: Long, name: String): Int

    /** mpv_stream_cb_add_ro */
    @JvmStatic external fun addStreamProtocol(handle: Long, protocol: String, adapter: ProtocolAdapter)
}
