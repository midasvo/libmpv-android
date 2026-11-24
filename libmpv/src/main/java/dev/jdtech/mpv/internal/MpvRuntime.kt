package dev.jdtech.mpv.internal

import dev.jdtech.mpv.MPVLib
import dev.jdtech.mpv.MPVLib.MpvEvent
import dev.jdtech.mpv.MpvException
import dev.jdtech.mpv.MpvNode
import dev.jdtech.mpv.MpvPlayerEvent
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CopyOnWriteArraySet
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow

/**
 * The per-instance state behind the node, async and flow API: the requests waiting for a
 * reply, the property observations, and the event-flow collectors, all keyed by the
 * `reply_userdata` ids mpv hands back.
 *
 * Ids come from one counter and are never reused, and are positive: libplayer gives
 * [MPVLib.observeProperty]'s observations negative ones, and delivers those to the
 * [MPVLib.EventObserver] callbacks as before.
 *
 * The `on*` methods run on mpv's event thread and never block: they complete a deferred,
 * offer to a conflated channel, or append to a bounded queue.
 */
internal class MpvRuntime(
    private val handleOf: () -> Long,
    private val native: RuntimeNative = RuntimeNative.Default,
) {
    private val nextId = AtomicLong(0)

    @Volatile
    private var destroyed = false

    private fun newId(): Long = nextId.incrementAndGet()

    private fun destroyedException() =
        IllegalStateException("MPVLib was destroyed while the call was pending")

    // ------------------------------------------------------------ async requests

    private class Request(val operation: String, val reply: CompletableDeferred<MpvNode>)

    private val requests = ConcurrentHashMap<Long, Request>()

    /** Replies that came after their request was cancelled, for the tests. */
    val ignoredReplies = AtomicInteger()

    /**
     * Sends one asynchronous request with a fresh id and suspends until its reply. If the
     * coroutine is cancelled first, the request is forgotten (its reply will be ignored)
     * and, when [abortable], aborted with mpv_abort_async_command.
     */
    suspend fun request(operation: String, abortable: Boolean, submit: (handle: Long, id: Long) -> Unit): MpvNode {
        val handle = handleOf()
        val id = newId()
        val request = Request(operation, CompletableDeferred())
        // registered before it is sent, so that onDestroyed finds every request it must fail
        requests[id] = request
        if (destroyed) {
            requests.remove(id)
            throw destroyedException()
        }
        try {
            submit(handle, id)
        } catch (t: Throwable) {
            requests.remove(id)
            throw t
        }
        try {
            return request.reply.await()
        } catch (e: CancellationException) {
            if (requests.remove(id) != null && abortable) {
                try {
                    native.abortAsyncCommand(handle, id)
                } catch (_: IllegalStateException) {
                    // destroyed meanwhile: nothing left to abort
                }
            }
            throw e
        }
    }

    /** MPV_EVENT_GET_PROPERTY_REPLY, _SET_PROPERTY_REPLY and _COMMAND_REPLY. */
    fun onReply(id: Long, error: Int, result: ByteArray?) {
        val request = requests.remove(id)
        if (request == null) {
            ignoredReplies.incrementAndGet() // its coroutine was cancelled
            return
        }
        if (error < 0) {
            request.reply.completeExceptionally(MpvException(error, request.operation))
            return
        }
        try {
            request.reply.complete(result?.let(NodeCodec::decode) ?: MpvNode.None)
        } catch (e: IllegalArgumentException) {
            request.reply.completeExceptionally(e)
        }
    }

    // ------------------------------------------------------------ property flows

    private class Observation {
        // latest value only: a slow collector skips values, it never holds up mpv
        val values = Channel<Any?>(Channel.CONFLATED)
    }

    private val observations = ConcurrentHashMap<Long, Observation>()

    /** How many observations mpv_unobserve_property removed, for the tests. */
    val unobserved = AtomicInteger()

    /**
     * A cold flow of property [name] observed with mpv [format]. Each collection observes
     * with its own id from the moment it starts (synchronously, before its first
     * suspension) until it ends, when it unobserves. [convert] turns what libplayer
     * delivers (Boolean, Long, Double, String, an encoded node, or null) into [T]; it runs
     * in the collector.
     */
    fun <T> propertyFlow(name: String, format: Int, convert: (Any?) -> T): Flow<T> = flow {
        val handle = handleOf()
        val id = newId()
        val observation = Observation()
        observations[id] = observation
        try {
            if (destroyed) return@flow
            native.observeProperty(handle, id, name, format)
            try {
                for (value in observation.values) emit(convert(value))
            } finally {
                try {
                    val removed = native.unobserveProperty(handle, id)
                    if (removed > 0) unobserved.addAndGet(removed)
                } catch (_: IllegalStateException) {
                    // destroyed: mpv observes nothing anymore
                }
            }
        } finally {
            observations.remove(id)
        }
    }

    /** MPV_EVENT_PROPERTY_CHANGE for a property flow. */
    fun onPropertyChange(id: Long, value: Any?) {
        observations[id]?.values?.trySend(value)
    }

    // ------------------------------------------------------------ event flow

    private val eventQueues = CopyOnWriteArraySet<EventQueue>()

    fun eventFlow(): Flow<MpvPlayerEvent> = flow {
        handleOf()
        val queue = EventQueue(EVENT_BUFFER)
        eventQueues.add(queue)
        try {
            if (destroyed) return@flow
            while (true) emit(queue.take() ?: break)
        } finally {
            eventQueues.remove(queue)
        }
    }

    /** Every event except log messages, property changes and replies. */
    fun onEvent(
        eventId: Int,
        entryId: Long,
        reason: Int,
        error: Int,
        insertId: Long,
        insertCount: Int,
        args: Array<String>?,
    ) {
        if (eventQueues.isEmpty()) return
        val event = when (eventId) {
            MpvEvent.MPV_EVENT_START_FILE -> MpvPlayerEvent.StartFile(entryId)
            MpvEvent.MPV_EVENT_FILE_LOADED -> MpvPlayerEvent.FileLoaded
            MpvEvent.MPV_EVENT_END_FILE ->
                MpvPlayerEvent.EndFile(reason, error, entryId, insertId, insertCount)
            MpvEvent.MPV_EVENT_CLIENT_MESSAGE -> MpvPlayerEvent.ClientMessage(args?.toList().orEmpty())
            MpvEvent.MPV_EVENT_SEEK -> MpvPlayerEvent.Seek
            MpvEvent.MPV_EVENT_PLAYBACK_RESTART -> MpvPlayerEvent.PlaybackRestart
            MpvEvent.MPV_EVENT_VIDEO_RECONFIG -> MpvPlayerEvent.VideoReconfig
            MpvEvent.MPV_EVENT_AUDIO_RECONFIG -> MpvPlayerEvent.AudioReconfig
            MpvEvent.MPV_EVENT_SHUTDOWN -> MpvPlayerEvent.Shutdown
            MpvEvent.MPV_EVENT_QUEUE_OVERFLOW -> MpvPlayerEvent.QueueOverflow
            else -> MpvPlayerEvent.Other(eventId)
        }
        for (queue in eventQueues) queue.offer(event)
    }

    // ------------------------------------------------------------ destroy

    /**
     * After mpv_terminate_destroy: the event thread is gone, so no reply or event comes
     * anymore. Pending requests fail with IllegalStateException; flows complete.
     */
    fun onDestroyed() {
        destroyed = true
        for (id in requests.keys.toList()) {
            requests.remove(id)?.reply?.completeExceptionally(destroyedException())
        }
        for (observation in observations.values) observation.values.close()
        for (queue in eventQueues) queue.close()
    }

    companion object {
        /** Events buffered per event-flow collector before it is told it lost some. */
        const val EVENT_BUFFER = 256
    }
}

/**
 * One event-flow collector's queue. [offer] never blocks: beyond [capacity] events the
 * newest are counted instead of stored, and delivered as one [MpvPlayerEvent.Dropped] at
 * the place they were lost, so the collector learns exactly where it fell behind.
 */
internal class EventQueue(private val capacity: Int) {
    private val lock = Any()
    private val events = ArrayDeque<MpvPlayerEvent>()
    private var closed = false

    // wakes the collector; conflated, so offering never waits for it
    private val signal = Channel<Unit>(Channel.CONFLATED)

    fun offer(event: MpvPlayerEvent) {
        synchronized(lock) {
            if (closed) return
            if (events.size < capacity) {
                events.addLast(event)
            } else {
                val last = events.lastOrNull()
                if (last is MpvPlayerEvent.Dropped) {
                    events[events.size - 1] = MpvPlayerEvent.Dropped(last.count + 1)
                } else {
                    events.addLast(MpvPlayerEvent.Dropped(1))
                }
            }
        }
        signal.trySend(Unit)
    }

    /** Ends the flow once the events already queued are taken. */
    fun close() {
        synchronized(lock) { closed = true }
        signal.trySend(Unit)
    }

    /** The next event, or null once closed and empty. */
    suspend fun take(): MpvPlayerEvent? {
        while (true) {
            synchronized(lock) {
                events.removeFirstOrNull()?.let { return it }
                if (closed) return null
            }
            signal.receive()
        }
    }
}

/** The native calls [MpvRuntime] makes itself, replaceable in the host unit tests. */
internal interface RuntimeNative {
    fun observeProperty(handle: Long, id: Long, name: String, format: Int)
    fun unobserveProperty(handle: Long, id: Long): Int
    fun abortAsyncCommand(handle: Long, id: Long)

    object Default : RuntimeNative {
        override fun observeProperty(handle: Long, id: Long, name: String, format: Int) =
            Native.observeProperty(handle, id, name, format)

        override fun unobserveProperty(handle: Long, id: Long): Int = Native.unobserveProperty(handle, id)

        override fun abortAsyncCommand(handle: Long, id: Long) = Native.abortAsyncCommand(handle, id)
    }
}
