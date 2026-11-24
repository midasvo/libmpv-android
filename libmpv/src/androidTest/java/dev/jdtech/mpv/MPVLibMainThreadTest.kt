package dev.jdtech.mpv

import android.os.Handler
import android.os.Looper
import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.jdtech.mpv.ApiTestSupport.load
import dev.jdtech.mpv.ApiTestSupport.test
import dev.jdtech.mpv.ApiTestSupport.withWavFile
import java.io.IOException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The suspend functions called on the main thread (`Dispatchers.Main`), as their KDoc says
 * they may be: while mpv has not answered yet, the main thread is free to run other work,
 * and the call resumes on the main thread when the answer comes.
 *
 * Each test makes the answer wait for something the test controls, so that "pending" lasts
 * as long as the test needs: mpv's event thread held in an observer callback (the reply is
 * an event, so it cannot be delivered), or a custom stream whose read blocks inside the
 * command (mpv itself is busy, as when it opens a slow file). Meanwhile a Runnable posted to
 * the main Looper from another thread must run: a call that blocked the main thread until
 * its answer would keep it from running until the test releases the answer, which it does
 * only after the Runnable ran, so the test would fail on the timeout instead.
 */
@RunWith(AndroidJUnit4::class)
class MPVLibMainThreadTest {
    private val mainThread: Thread = Looper.getMainLooper().thread

    /**
     * Holds mpv's event thread in an observer callback until the returned latch is counted
     * down, and returns once it is held. Nothing the event thread delivers (replies included)
     * gets through meanwhile; requests still reach mpv.
     */
    private fun holdEventThread(mpv: MPVLib): CountDownLatch {
        val holding = CountDownLatch(1)
        val release = CountDownLatch(1)
        mpv.addObserver(object : MPVLib.EventObserver {
            override fun eventProperty(property: String, value: Double) {
                if (property == "volume" && holding.count > 0) {
                    holding.countDown()
                    release.await(TestMpv.TIMEOUT_SECONDS * 2, TimeUnit.SECONDS)
                }
            }
        })
        mpv.observeProperty("volume", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
        ApiTestSupport.await(holding, "the event thread to be held")
        return release
    }

    /**
     * Runs [call] in a coroutine on `Dispatchers.Main`. Once it has started there (and
     * [whilePending] has returned, for a test that waits for mpv to be busy), checks that the
     * main thread runs a posted Runnable while the call is still pending; then [release]s the
     * answer (always, also when a check fails) and returns the call's result, checking that
     * it resumed on the main thread.
     */
    private fun <T> CoroutineScope.onMainWhilePending(
        release: () -> Unit,
        whilePending: () -> Unit = {},
        call: suspend () -> T,
    ): kotlinx.coroutines.Deferred<T> {
        val started = CountDownLatch(1)
        val resumedOn = AtomicReference<Thread?>()
        val deferred = async(Dispatchers.Main) {
            started.countDown()
            call().also { resumedOn.set(Thread.currentThread()) }
        }
        try {
            ApiTestSupport.await(started, "the call to start on the main thread")
            whilePending()
            val ran = CountDownLatch(1)
            Handler(Looper.getMainLooper()).post { ran.countDown() }
            ApiTestSupport.await(ran, "the main thread to run a Runnable while the call was pending")
            assertFalse("the call completed before its answer was released", deferred.isCompleted)
        } finally {
            release()
        }
        return async {
            val result = deferred.await()
            assertSame("resumed on the main thread", mainThread, resumedOn.get())
            result
        }
    }

    @Test
    fun getPropertyAsyncLeavesTheMainThreadFree() = TestMpv.withMpv { mpv ->
        val release = holdEventThread(mpv)
        test {
            val version = onMainWhilePending(release = { release.countDown() }) {
                assertSame(mainThread, Thread.currentThread())
                mpv.getPropertyAsync("mpv-version")
            }.await()
            assertTrue("$version", version.string!!.startsWith("mpv "))
        }
    }

    @Test
    fun setPropertyAsyncLeavesTheMainThreadFree() = TestMpv.withMpv { mpv ->
        val release = holdEventThread(mpv)
        test {
            onMainWhilePending(release = { release.countDown() }) {
                assertSame(mainThread, Thread.currentThread())
                mpv.setPropertyAsync("speed", MpvNode.DoubleValue(1.25))
            }.await()
            assertEquals(1.25, mpv.getPropertyDouble("speed")!!, 0.0)
        }
    }

    @Test
    fun commandAsyncLeavesTheMainThreadFree() = TestMpv.withMpv { mpv ->
        val release = holdEventThread(mpv)
        test {
            val text = onMainWhilePending(release = { release.countDown() }) {
                assertSame(mainThread, Thread.currentThread())
                mpv.commandAsync(MpvNode.command("expand-text", "main=\${speed}"))
            }.await()
            assertTrue("$text", text.string!!.startsWith("main="))
        }
    }

    /** The case the main-thread advice is about: mpv itself busy (opening a slow stream) inside the command. */
    @Test
    fun commandAsyncLeavesTheMainThreadFreeWhileMpvIsBusy() =
        withWavFile("libmpv-test-main-thread.wav", 1.0) { file ->
            TestMpv.withMpv("pause" to "yes") { mpv ->
                val gate = CountDownLatch(1)
                val stream = GatedStream(ApiTestSupport.wavBytes(0.5), gate)
                mpv.registerStreamProtocol("libmpvtestgated") { stream }
                test {
                    load(mpv, file.absolutePath)
                    onMainWhilePending(
                        release = { gate.countDown() },
                        // mpv's core is stuck in the stream's read, inside audio-add
                        whilePending = { ApiTestSupport.await(stream.reading, "mpv to read the gated stream") },
                    ) {
                        assertSame(mainThread, Thread.currentThread())
                        mpv.commandAsync(MpvNode.command("audio-add", "libmpvtestgated://audio"))
                    }.await()
                    val audio = mpv.getPropertyNode("track-list").list.orEmpty().filter { it["type"]?.string == "audio" }
                    assertEquals("$audio", 2, audio.size)
                }
            }
        }

    /** A seekable stream over [data] whose reads all wait until [gate] opens. */
    private class GatedStream(private val data: ByteArray, private val gate: CountDownLatch) : MpvStream {
        val reading = CountDownLatch(1)

        @Volatile private var position = 0

        override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
            reading.countDown()
            if (!gate.await(TestMpv.TIMEOUT_SECONDS * 2, TimeUnit.SECONDS)) throw IOException("never released")
            if (position >= data.size) return -1
            val n = minOf(length, data.size - position)
            System.arraycopy(data, position, buffer, offset, n)
            position += n
            return n
        }

        override fun seek(position: Long): Long {
            if (position > data.size) throw IOException("past the end")
            this.position = position.toInt()
            return position
        }

        override fun size(): Long = data.size.toLong()

        override fun close() {}
    }
}
