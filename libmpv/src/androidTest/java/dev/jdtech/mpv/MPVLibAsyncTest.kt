package dev.jdtech.mpv

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.jdtech.mpv.ApiTestSupport.eventually
import dev.jdtech.mpv.ApiTestSupport.load
import dev.jdtech.mpv.ApiTestSupport.test
import dev.jdtech.mpv.ApiTestSupport.withWavFile
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** The suspend functions on mpv's async API: results, errors, cancellation, destroy(). */
@RunWith(AndroidJUnit4::class)
class MPVLibAsyncTest {

    private suspend fun <T> mpvError(block: suspend () -> T): Int =
        try {
            block()
            throw AssertionError("no MpvException")
        } catch (e: MpvException) {
            e.error
        }

    @Test
    fun getSetAndCommand() = TestMpv.withMpv { mpv ->
        test {
            assertTrue(mpv.getPropertyAsync("mpv-version").string!!.startsWith("mpv "))
            mpv.setPropertyAsync("speed", MpvNode.DoubleValue(1.5))
            assertEquals(MpvNode.DoubleValue(1.5), mpv.getPropertyAsync("speed"))
            val expanded = mpv.commandAsync(MpvNode.command("expand-text", "\${speed}")).string!!
            assertEquals(expanded, 1.5, expanded.toDouble(), 0.0)
            assertEquals(MpvNode.None, mpv.commandAsync(MpvNode.command("playlist-clear")))
            // the sync API sees the same instance
            assertEquals(1.5, mpv.getPropertyDouble("speed")!!, 0.0)
        }
    }

    @Test
    fun manyConcurrentCallsGetTheirOwnReplies() = TestMpv.withMpv { mpv ->
        test {
            val calls = (1..100).map { i ->
                async { mpv.commandAsync(MpvNode.command("expand-text", "n=$i")) }
            }
            calls.forEachIndexed { i, call -> assertEquals(MpvNode.StringValue("n=${i + 1}"), call.await()) }
        }
    }

    @Test
    fun theMainThreadIsNotNeeded() = TestMpv.withMpv { mpv ->
        // resumed in the caller's context, whatever thread mpv answered on
        test {
            withContext(Dispatchers.Default) {
                mpv.getPropertyAsync("mpv-version")
                val resumedOn = Thread.currentThread().name
                assertTrue(resumedOn, resumedOn.contains("DefaultDispatcher"))
                assertNotEquals("event_thread", resumedOn)
            }
        }
    }

    @Test
    fun mpvErrorsAreTypedFailures() = TestMpv.withMpv { mpv ->
        test {
            // errors in mpv's reply
            assertEquals(
                MPVLib.MpvError.MPV_ERROR_PROPERTY_NOT_FOUND,
                mpvError { mpv.getPropertyAsync("libmpv-test-no-such-property") },
            )
            assertEquals(
                MPVLib.MpvError.MPV_ERROR_PROPERTY_UNAVAILABLE,
                mpvError { mpv.getPropertyAsync("duration") },
            )
            assertTrue(mpvError { mpv.setPropertyAsync("speed", MpvNode.StringValue("fast")) } < 0)
            // and an error queuing the request: mpv parses the command before it returns
            assertTrue(mpvError { mpv.commandAsync(MpvNode.command("libmpv-test-no-such-command")) } < 0)
            // the instance is fine afterwards
            assertTrue(mpv.getPropertyAsync("mpv-version").string!!.startsWith("mpv "))
        }
    }

    /**
     * A command that runs until it is aborted: `audio-add` of a stream whose reads block
     * until mpv cancels them. Cancelling the coroutine must return at once and abort the
     * command in mpv (mpv_abort_async_command): the stream is cancelled, and mpv's reply to
     * the aborted command arrives (and is ignored) long before the read would give up.
     */
    @Test
    fun cancellingAbortsTheCommandInMpv() = withWavFile("libmpv-test-async.wav", 2.0) { file ->
        TestMpv.withMpv("pause" to "yes") { mpv ->
            val stalled = RecordingStream(ByteArray(0), blockReads = true)
            mpv.registerStreamProtocol("libmpvtestslow") { stalled }
            test {
                load(mpv, file.absolutePath)
                val ignoredBefore = mpv.runtime.ignoredReplies.get()
                val call = launch(start = CoroutineStart.UNDISPATCHED) {
                    mpv.commandAsync(MpvNode.command("audio-add", "libmpvtestslow://stalled"))
                }
                withContext(Dispatchers.IO) { ApiTestSupport.await(stalled.readBlocked, "the stalled read") }
                assertFalse(call.isCompleted)
                call.cancel()
                call.join()
                assertTrue(call.isCancelled)
                withContext(Dispatchers.IO) { ApiTestSupport.await(stalled.cancelled, "the stream's cancel") }
                eventually { mpv.runtime.ignoredReplies.get() > ignoredBefore }
                withContext(Dispatchers.IO) { ApiTestSupport.await(stalled.closed, "the stream's close") }
                // still usable
                assertEquals("wav", mpv.getPropertyAsync("file-format").string)
            }
        }
    }

    @Test
    fun cancellingAReadOnlyStopsWaiting() = TestMpv.withMpv { mpv ->
        // Hold mpv's event thread in an observer callback, so that the reply cannot be
        // delivered before the cancellation (the request still reaches mpv meanwhile).
        val holding = CountDownLatch(1)
        val release = CountDownLatch(1)
        mpv.addObserver(object : MPVLib.EventObserver {
            override fun eventProperty(property: String, value: Double) {
                if (property == "volume" && holding.count > 0) {
                    holding.countDown()
                    release.await(TestMpv.TIMEOUT_SECONDS, TimeUnit.SECONDS)
                }
            }
        })
        mpv.observeProperty("volume", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
        ApiTestSupport.await(holding, "the event thread to be held")
        test {
            val ignoredBefore = mpv.runtime.ignoredReplies.get()
            val call = async(start = CoroutineStart.UNDISPATCHED) { mpv.getPropertyAsync("mpv-version") }
            assertFalse("no reply while the event thread is held", call.isCompleted)
            call.cancel()
            assertTrue(runCatching { call.await() }.exceptionOrNull() is kotlinx.coroutines.CancellationException)
            release.countDown()
            // its reply comes anyway and is dropped; the next call is unaffected
            eventually { mpv.runtime.ignoredReplies.get() > ignoredBefore }
            assertTrue(mpv.getPropertyAsync("mpv-version").string!!.startsWith("mpv "))
        }
    }

    @Test
    fun destroyWhileACallIsPendingFailsIt(): Unit = withWavFile("libmpv-test-async-destroy.wav", 2.0) { file ->
        val mpv = TestMpv.create("pause" to "yes")
        try {
            val stalled = RecordingStream(ByteArray(0), blockReads = true)
            mpv.registerStreamProtocol("libmpvtestslow") { stalled }
            val failure = AtomicReference<Throwable?>()
            test {
                load(mpv, file.absolutePath)
                val call = launch(Dispatchers.Default, start = CoroutineStart.UNDISPATCHED) {
                    try {
                        mpv.commandAsync(MpvNode.command("audio-add", "libmpvtestslow://stalled"))
                    } catch (e: Throwable) {
                        failure.set(e)
                    }
                }
                withContext(Dispatchers.IO) {
                    ApiTestSupport.await(stalled.readBlocked, "the stalled read")
                    mpv.destroy() // aborts mpv's work, closes the stream, and fails the call
                }
                call.join()
                withContext(Dispatchers.IO) { ApiTestSupport.await(stalled.closed, "the stream's close") }
            }
            val e = failure.get()
            assertTrue("pending call ended with $e", e is IllegalStateException)
            assertTrue(e!!.message!!.contains("destroyed"))
        } finally {
            mpv.destroy()
        }
        // and after destroy, calls fail at once
        test {
            assertThrows(IllegalStateException::class.java) {
                kotlinx.coroutines.runBlocking { mpv.getPropertyAsync("mpv-version") }
            }
        }
    }
}
