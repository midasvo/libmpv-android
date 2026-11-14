package dev.jdtech.mpv

import android.content.Context
import android.view.Surface
import java.util.concurrent.CopyOnWriteArrayList

@Suppress("unused")
class MPVLib private constructor(nativePtr: Long) {
    private var nativeInstance: Long = 0
    // Copy-on-write so observers may add or remove observers from inside a callback.
    private val observers = CopyOnWriteArrayList<EventObserver>()
    private val logObservers = CopyOnWriteArrayList<LogObserver>()

    companion object {
        init {
            val libs = arrayOf("mpv", "player")
            for (lib in libs) {
                System.loadLibrary(lib)
            }
        }

        @JvmStatic
        fun create(context: Context): MPVLib? {
            val appCtx = context.applicationContext

            val instance = MPVLib(0L)

            val ptr = instance.nativeCreate(instance, appCtx)
            if (ptr == 0L) {
                return null
            }

            instance.nativeInstance = ptr
            return instance
        }
    }

    private fun checkCreated() {
        if (nativeInstance == 0L) {
            throw IllegalStateException("MPVLib is not initialized")
        }
    }

    private external fun nativeCreate(thiz: MPVLib, appctx: Context): Long

    fun init() {
        checkCreated()
        nativeInit(nativeInstance)
    }
    private external fun nativeInit(instance: Long)

    fun destroy() {
        if (nativeInstance != 0L) {
            nativeDestroy(nativeInstance)
            nativeInstance = 0L
        }
    }
    private external fun nativeDestroy(instance: Long)

    fun attachSurface(surface: Surface) {
        checkCreated()
        nativeAttachSurface(nativeInstance, surface)
    }
    private external fun nativeAttachSurface(instance: Long, surface: Surface)

    fun detachSurface() {
        checkCreated()
        nativeDetachSurface(nativeInstance)
    }
    private external fun nativeDetachSurface(instance: Long)

    /** Runs an mpv command and returns the mpv error code (0 on success, negative on failure). */
    fun command(cmd: Array<String>): Int {
        checkCreated()
        return nativeCommand(nativeInstance, cmd)
    }
    private external fun nativeCommand(instance: Long, cmd: Array<String>): Int

    /**
     * Sets the minimum level of mpv log messages delivered to [LogObserver]s
     * and echoed to logcat ("no", "fatal", "error", "warn", "info", "v",
     * "debug", "trace"). Defaults to "v". Verbose messages include the URLs
     * mpv opens, so apps playing authenticated URLs may want "warn" or "no".
     * Returns an mpv error code (0 on success).
     */
    fun requestLogMessages(level: String): Int {
        checkCreated()
        return nativeRequestLogMessages(nativeInstance, level)
    }
    private external fun nativeRequestLogMessages(instance: Long, level: String): Int

    fun setOptionString(name: String, value: String): Int {
        checkCreated()
        return nativeSetOptionString(nativeInstance, name, value)
    }
    private external fun nativeSetOptionString(instance: Long, name: String, value: String): Int

    /** Reads an int64 property truncated to 32 bits (kept for compatibility); use [getPropertyLong] for the full value. */
    fun getPropertyInt(property: String): Int? {
        checkCreated()
        return nativeGetPropertyInt(nativeInstance, property)
    }
    private external fun nativeGetPropertyInt(instance: Long, property: String): Int?

    /** Reads an int64 property without truncation. */
    fun getPropertyLong(property: String): Long? {
        checkCreated()
        return nativeGetPropertyLong(nativeInstance, property)
    }
    private external fun nativeGetPropertyLong(instance: Long, property: String): Long?

    fun setPropertyInt(property: String, value: Int) {
        checkCreated()
        nativeSetPropertyInt(nativeInstance, property, value)
    }
    private external fun nativeSetPropertyInt(instance: Long, property: String, value: Int)

    fun getPropertyDouble(property: String): Double? {
        checkCreated()
        return nativeGetPropertyDouble(nativeInstance, property)
    }
    private external fun nativeGetPropertyDouble(instance: Long, property: String): Double?

    fun setPropertyDouble(property: String, value: Double) {
        checkCreated()
        nativeSetPropertyDouble(nativeInstance, property, value)
    }
    private external fun nativeSetPropertyDouble(instance: Long, property: String, value: Double)

    fun getPropertyBoolean(property: String): Boolean? {
        checkCreated()
        return nativeGetPropertyBoolean(nativeInstance, property)
    }
    private external fun nativeGetPropertyBoolean(instance: Long, property: String): Boolean?

    fun setPropertyBoolean(property: String, value: Boolean) {
        checkCreated()
        nativeSetPropertyBoolean(nativeInstance, property, value)
    }
    private external fun nativeSetPropertyBoolean(instance: Long, property: String, value: Boolean)

    fun getPropertyString(property: String): String? {
        checkCreated()
        return nativeGetPropertyString(nativeInstance, property)
    }
    private external fun nativeGetPropertyString(instance: Long, property: String): String?

    fun setPropertyString(property: String, value: String) {
        checkCreated()
        nativeSetPropertyString(nativeInstance, property, value)
    }
    private external fun nativeSetPropertyString(instance: Long, property: String, value: String)

    fun observeProperty(property: String, format: Int) {
        checkCreated()
        nativeObserveProperty(nativeInstance, property, format)
    }
    private external fun nativeObserveProperty(instance: Long, property: String, format: Int)

    fun addObserver(o: EventObserver) {
        observers.add(o)
    }

    fun removeObserver(o: EventObserver) {
        observers.remove(o)
    }

    fun eventProperty(property: String, value: Long) {
        for (o in observers)
            o.eventProperty(property, value)
    }

    fun eventProperty(property: String, value: Double) {
        for (o in observers)
            o.eventProperty(property, value)
    }

    fun eventProperty(property: String, value: Boolean) {
        for (o in observers)
            o.eventProperty(property, value)
    }

    fun eventProperty(property: String, value: String) {
        for (o in observers)
            o.eventProperty(property, value)
    }

    fun eventProperty(property: String) {
        for (o in observers)
            o.eventProperty(property)
    }

    fun event(eventId: Int) {
        for (o in observers)
            o.event(eventId)
    }

    fun addLogObserver(o: LogObserver) {
        logObservers.add(o)
    }

    fun removeLogObserver(o: LogObserver) {
        logObservers.remove(o)
    }

    fun logMessage(prefix: String, level: Int, text: String) {
        for (o in logObservers)
            o.logMessage(prefix, level, text)
    }

    interface EventObserver {
        fun eventProperty(property: String)
        fun eventProperty(property: String, value: Long)
        fun eventProperty(property: String, value: Double)
        fun eventProperty(property: String, value: Boolean)
        fun eventProperty(property: String, value: String)
        fun event(eventId: Int)
    }

    interface LogObserver {
        fun logMessage(prefix: String, level: Int, text: String)
    }

    object MpvFormat {
        const val MPV_FORMAT_NONE: Int = 0
        const val MPV_FORMAT_STRING: Int = 1
        const val MPV_FORMAT_OSD_STRING: Int = 2
        const val MPV_FORMAT_FLAG: Int = 3
        const val MPV_FORMAT_INT64: Int = 4
        const val MPV_FORMAT_DOUBLE: Int = 5
        const val MPV_FORMAT_NODE: Int = 6
        const val MPV_FORMAT_NODE_ARRAY: Int = 7
        const val MPV_FORMAT_NODE_MAP: Int = 8
        const val MPV_FORMAT_BYTE_ARRAY: Int = 9
    }

    object MpvEvent {
        const val MPV_EVENT_NONE: Int = 0
        const val MPV_EVENT_SHUTDOWN: Int = 1
        const val MPV_EVENT_LOG_MESSAGE: Int = 2
        const val MPV_EVENT_GET_PROPERTY_REPLY: Int = 3
        const val MPV_EVENT_SET_PROPERTY_REPLY: Int = 4
        const val MPV_EVENT_COMMAND_REPLY: Int = 5
        const val MPV_EVENT_START_FILE: Int = 6
        const val MPV_EVENT_END_FILE: Int = 7
        const val MPV_EVENT_FILE_LOADED: Int = 8
        @Deprecated("")
        const val MPV_EVENT_IDLE: Int = 11
        @Deprecated("")
        const val MPV_EVENT_TICK: Int = 14
        const val MPV_EVENT_CLIENT_MESSAGE: Int = 16
        const val MPV_EVENT_VIDEO_RECONFIG: Int = 17
        const val MPV_EVENT_AUDIO_RECONFIG: Int = 18
        const val MPV_EVENT_SEEK: Int = 20
        const val MPV_EVENT_PLAYBACK_RESTART: Int = 21
        const val MPV_EVENT_PROPERTY_CHANGE: Int = 22
        const val MPV_EVENT_QUEUE_OVERFLOW: Int = 24
        const val MPV_EVENT_HOOK: Int = 25
    }

    object MpvLogLevel {
        const val MPV_LOG_LEVEL_NONE: Int = 0
        const val MPV_LOG_LEVEL_FATAL: Int = 10
        const val MPV_LOG_LEVEL_ERROR: Int = 20
        const val MPV_LOG_LEVEL_WARN: Int = 30
        const val MPV_LOG_LEVEL_INFO: Int = 40
        const val MPV_LOG_LEVEL_V: Int = 50
        const val MPV_LOG_LEVEL_DEBUG: Int = 60
        const val MPV_LOG_LEVEL_TRACE: Int = 70
    }
}