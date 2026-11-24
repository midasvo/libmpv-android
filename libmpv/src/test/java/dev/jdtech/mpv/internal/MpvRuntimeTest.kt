package dev.jdtech.mpv.internal

import dev.jdtech.mpv.MPVLib.MpvError
import dev.jdtech.mpv.MPVLib.MpvEvent
import dev.jdtech.mpv.MPVLib.MpvFormat
import dev.jdtech.mpv.MpvException
import dev.jdtech.mpv.MpvNode
import dev.jdtech.mpv.MpvPlayerEvent
import java.util.Collections
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.yield
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * The request and observation bookkeeping, with the native side replaced by a recorder:
 * replies, property changes and events are delivered by hand, as libplayer's event thread
 * would.
 */
class MpvRuntimeTest {
    private class FakeNative : RuntimeNative {
        val calls: MutableList<String> = Collections.synchronizedList(mutableListOf())
        var observeError: Int? = null
        override fun observeProperty(handle: Long, id: Long, name: String, format: Int) {
            calls += "observe $id $name $format"
            observeError?.let { throw MpvException(it, "observe") }
        }

        override fun unobserveProperty(handle: Long, id: Long): Int {
            calls += "unobserve $id"
            return 1
        }

        override fun abortAsyncCommand(handle: Long, id: Long) {
            calls += "abort $id"
        }
    }

    private val native = FakeNative()
    private val runtime = MpvRuntime({ 7L }, native)

    private fun <T> run(block: suspend kotlinx.coroutines.CoroutineScope.() -> T): T =
        runBlocking { withTimeout(10_000) { block() } }

    @Test
    fun aReplyCompletesItsRequest() = run {
        val sent = Channel<Long>(Channel.UNLIMITED)
        val call = async(start = CoroutineStart.UNDISPATCHED) {
            runtime.request("op", abortable = true) { handle, id ->
                assertEquals(7L, handle)
                sent.trySend(id)
            }
        }
        val id = sent.receive()
        assertTrue("ids are positive", id > 0)
        runtime.onReply(id + 1000, 0, null) // someone else's: ignored
        runtime.onReply(id, 0, NodeCodec.encode(MpvNode.StringValue("done")))
        assertEquals(MpvNode.StringValue("done"), call.await())
        runtime.onReply(id, 0, null) // a second reply: ignored
    }

    @Test
    fun aReplyWithoutResultIsNone() = run {
        var id = 0L
        val call = async(start = CoroutineStart.UNDISPATCHED) {
            runtime.request("op", abortable = false) { _, i -> id = i }
        }
        runtime.onReply(id, 0, null)
        assertEquals(MpvNode.None, call.await())
    }

    @Test
    fun mpvsErrorIsThrown() = run {
        var id = 0L
        val call = async(start = CoroutineStart.UNDISPATCHED) {
            runCatching { runtime.request("getPropertyAsync(x)", abortable = false) { _, i -> id = i } }
        }
        runtime.onReply(id, MpvError.MPV_ERROR_PROPERTY_NOT_FOUND, null)
        val e = call.await().exceptionOrNull() as MpvException
        assertEquals(MpvError.MPV_ERROR_PROPERTY_NOT_FOUND, e.error)
        assertTrue(e.message!!.startsWith("getPropertyAsync(x) failed"))
    }

    @Test
    fun aRequestThatCannotBeSentFailsAtOnceAndLeavesNothingBehind() = run {
        var id = 0L
        val e = runCatching {
            runtime.request("op", abortable = true) { _, i ->
                id = i
                throw MpvException(MpvError.MPV_ERROR_INVALID_PARAMETER, "op")
            }
        }.exceptionOrNull()
        assertTrue(e is MpvException)
        runtime.onReply(id, 0, null) // nothing waits for it
        assertTrue(native.calls.isEmpty())
    }

    @Test
    fun cancellingAbortsCommandsOnly() = run {
        val ids = mutableListOf<Long>()
        val command = launch(start = CoroutineStart.UNDISPATCHED) {
            runtime.request("command", abortable = true) { _, i -> ids += i }
        }
        val get = launch(start = CoroutineStart.UNDISPATCHED) {
            runtime.request("get", abortable = false) { _, i -> ids += i }
        }
        command.cancel()
        get.cancel()
        command.join()
        get.join()
        assertTrue(command.isCancelled && get.isCancelled)
        assertEquals(listOf("abort ${ids[0]}"), native.calls)
        // mpv still replies to an aborted command; nobody is waiting anymore
        runtime.onReply(ids[0], MpvError.MPV_ERROR_COMMAND, null)
        runtime.onReply(ids[1], 0, null)
        assertEquals(1, native.calls.size)
    }

    @Test
    fun destroyFailsPendingRequestsAndRefusesNewOnes() = run {
        val pending = async(start = CoroutineStart.UNDISPATCHED) {
            runCatching { runtime.request("op", abortable = true) { _, _ -> } }
        }
        runtime.onDestroyed()
        val e = pending.await().exceptionOrNull()
        assertTrue("$e", e is IllegalStateException)
        assertTrue(e!!.message!!.contains("destroyed"))
        var sent = false
        assertThrows(IllegalStateException::class.java) {
            runBlocking { runtime.request("op", abortable = true) { _, _ -> sent = true } }
        }
        assertTrue("nothing is sent after destroy", !sent)
    }

    @Test
    fun idsAreNeverReused() = run {
        val ids = mutableSetOf<Long>()
        repeat(100) {
            val call = async(start = CoroutineStart.UNDISPATCHED) {
                runtime.request("op", abortable = false) { _, i -> assertTrue(ids.add(i)) }
            }
            runtime.onReply(ids.last(), 0, null)
            call.await()
        }
        assertEquals(100, ids.size)
    }

    @Test
    fun aPropertyFlowObservesWhileCollectedAndUnobservesAfter() = run {
        val seen = CopyOnWriteArrayList<Double?>()
        val third = Channel<Unit>(1)
        val job = launch(start = CoroutineStart.UNDISPATCHED) {
            runtime.propertyFlow("volume", MpvFormat.MPV_FORMAT_DOUBLE) { it as Double? }.collect {
                seen += it
                if (seen.size == 3) third.trySend(Unit)
            }
        }
        // observing began before launch returned
        val observe = native.calls.single()
        val id = observe.split(' ')[1].toLong()
        assertEquals("observe $id volume ${MpvFormat.MPV_FORMAT_DOUBLE}", observe)
        assertTrue(id > 0)
        runtime.onPropertyChange(id, 50.0)
        yield()
        runtime.onPropertyChange(id + 1, 1.0) // another observation's
        runtime.onPropertyChange(id, null) // unavailable
        yield()
        runtime.onPropertyChange(id, 75.0)
        third.receive()
        assertEquals(listOf(50.0, null, 75.0), seen)
        job.cancel()
        job.join()
        assertEquals(listOf(observe, "unobserve $id"), native.calls)
        assertEquals(1, runtime.unobserved.get())
        runtime.onPropertyChange(id, 1.0) // after unobserve: dropped, no crash
    }

    @Test
    fun eachCollectionHasItsOwnObservation() = run {
        val flow = runtime.propertyFlow("pause", MpvFormat.MPV_FORMAT_FLAG) { it as Boolean? }
        val a = launch(start = CoroutineStart.UNDISPATCHED) { flow.collect { } }
        val b = launch(start = CoroutineStart.UNDISPATCHED) { flow.collect { } }
        val ids = native.calls.map { it.split(' ')[1] }
        assertEquals(2, ids.toSet().size)
        a.cancel()
        b.cancel()
        a.join()
        b.join()
        assertEquals(4, native.calls.size)
    }

    @Test
    fun aSlowCollectorGetsTheLatestValue() = run {
        val gate = Channel<Unit>()
        val values = CopyOnWriteArrayList<Long?>()
        val job = launch(start = CoroutineStart.UNDISPATCHED) {
            runtime.propertyFlow("playlist-pos", MpvFormat.MPV_FORMAT_INT64) { it as Long? }.collect {
                values += it
                gate.receive()
            }
        }
        val id = native.calls.single().split(' ')[1].toLong()
        runtime.onPropertyChange(id, 0L)
        yield() // the collector takes 0 and waits at the gate
        // the event thread is never held up, however many changes come meanwhile
        for (i in 1L..1000L) runtime.onPropertyChange(id, i)
        gate.send(Unit)
        gate.send(Unit)
        withTimeout(5_000) { while (values.size < 2) yield() }
        assertEquals(listOf(0L, 1000L), values)
        job.cancel()
    }

    @Test
    fun aFailedObservationIsAnErrorOfTheFlow() = run {
        native.observeError = MpvError.MPV_ERROR_NOMEM
        val e = runCatching {
            runtime.propertyFlow("x", MpvFormat.MPV_FORMAT_NODE) { it }.collect { }
        }.exceptionOrNull()
        assertEquals(MpvError.MPV_ERROR_NOMEM, (e as MpvException).error)
        assertEquals(1, native.calls.size) // nothing to unobserve
    }

    @Test
    fun destroyCompletesTheFlows() = run {
        val property = async(start = CoroutineStart.UNDISPATCHED) {
            runtime.propertyFlow("x", MpvFormat.MPV_FORMAT_STRING) { it as String? }.toList()
        }
        val events = async(start = CoroutineStart.UNDISPATCHED) { runtime.eventFlow().toList() }
        val id = native.calls.single().split(' ')[1].toLong()
        runtime.onPropertyChange(id, "a")
        runtime.onEvent(MpvEvent.MPV_EVENT_FILE_LOADED, 0, 0, 0, 0, 0, null)
        yield()
        runtime.onDestroyed()
        assertEquals(listOf("a"), property.await())
        assertEquals(listOf(MpvPlayerEvent.FileLoaded), events.await())
        // collecting after destroy ends at once, observing nothing
        assertEquals(emptyList<String?>(), runtime.propertyFlow("y", 1) { it }.toList())
        assertEquals(1, native.calls.count { it.startsWith("observe") })
    }

    @Test
    fun eventsCarryMpvsData() = run {
        val events = Channel<MpvPlayerEvent>(Channel.UNLIMITED)
        val job = launch(start = CoroutineStart.UNDISPATCHED) { runtime.eventFlow().collect { events.send(it) } }
        runtime.onEvent(MpvEvent.MPV_EVENT_START_FILE, 3, 0, 0, 0, 0, null)
        runtime.onEvent(MpvEvent.MPV_EVENT_END_FILE, 3, 4, -13, 0, 0, null)
        runtime.onEvent(MpvEvent.MPV_EVENT_CLIENT_MESSAGE, 0, 0, 0, 0, 0, arrayOf("a", "b"))
        runtime.onEvent(MpvEvent.MPV_EVENT_SEEK, 0, 0, 0, 0, 0, null)
        runtime.onEvent(99, 0, 0, 0, 0, 0, null)
        assertEquals(MpvPlayerEvent.StartFile(3), events.receive())
        assertEquals(MpvPlayerEvent.EndFile(4, -13, 3, 0, 0), events.receive())
        assertEquals(MpvPlayerEvent.ClientMessage(listOf("a", "b")), events.receive())
        assertEquals(MpvPlayerEvent.Seek, events.receive())
        assertEquals(MpvPlayerEvent.Other(99), events.receive())
        job.cancel()
    }



    @Test
    fun cancellationIsNotAnMpvError() = run {
        val job = async(start = CoroutineStart.UNDISPATCHED) {
            try {
                runtime.request("op", abortable = true) { _, _ -> }
                fail("returned")
            } catch (e: CancellationException) {
                assertNotEquals(MpvException::class.java, e.javaClass)
                throw e
            }
        }
        job.cancel()
        job.join()
        assertTrue(job.isCancelled)
    }
}
