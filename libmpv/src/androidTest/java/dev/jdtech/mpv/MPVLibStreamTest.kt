package dev.jdtech.mpv

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.jdtech.mpv.ApiTestSupport.SAMPLE_RATE
import dev.jdtech.mpv.ApiTestSupport.eventually
import dev.jdtech.mpv.ApiTestSupport.load
import dev.jdtech.mpv.ApiTestSupport.test
import dev.jdtech.mpv.ApiTestSupport.wavBytes
import java.io.IOException
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.dropWhile
import kotlinx.coroutines.flow.filterIsInstance
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import org.junit.runner.RunWith

/** Custom protocols (mpv_stream_cb_add_ro) implemented in Kotlin. */
@RunWith(AndroidJUnit4::class)
class MPVLibStreamTest {
    private companion object {
        const val PROTOCOL = "libmpvtest"
    }

    /** Opens every URI as a [RecordingStream] over [data], and keeps them. */
    private class Provider(
        private val data: ByteArray,
        private val failReadsAfter: Int = Int.MAX_VALUE,
        private val maxRead: Int = Int.MAX_VALUE,
    ) :
        MpvStreamProvider {
        val uris = CopyOnWriteArrayList<String>()
        val streams = CopyOnWriteArrayList<RecordingStream>()
        override fun open(uri: String): MpvStream {
            uris += uri
            return RecordingStream(data, failReadsAfter = failReadsAfter, maxRead = maxRead).also { streams += it }
        }
    }

    private fun endOf(mpv: MPVLib, block: suspend () -> Unit): MpvPlayerEvent.EndFile = test {
        val end = async(start = CoroutineStart.UNDISPATCHED) {
            mpv.eventFlow().filterIsInstance<MpvPlayerEvent.EndFile>().first()
        }
        block()
        end.await()
    }

    @Test
    fun playsAWavThroughACustomProtocolToTheEnd() = TestMpv.withMpv { mpv ->
        val provider = Provider(wavBytes(0.5))
        mpv.registerStreamProtocol(PROTOCOL, provider)
        val uri = "$PROTOCOL://tone/0.5s?x=1"
        val end = endOf(mpv) { mpv.command(arrayOf("loadfile", uri)) }
        assertEquals(MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_EOF, end.reason)
        assertEquals(0, end.error)
        assertTrue("opened ${provider.uris}", uri in provider.uris)

        // every stream mpv opened was closed, exactly once, with nothing after it
        for (stream in provider.streams) {
            ApiTestSupport.await(stream.closed, "close")
            assertEquals(stream.calls.toString(), 1, stream.calls.count { it == "close" })
            assertFalse(stream.calls.toString(), stream.calls.any { it.startsWith("AFTER CLOSE") })
            // mpv's own threads, never the test's
            assertFalse(stream.threads.toString(), Thread.currentThread() in stream.threads)
        }
        val main = provider.streams.first { s -> s.calls.count { it == "read" } > 1 }
        assertEquals("seekability is probed first", "seek 0", main.calls.first { it != "size" })
    }

    @Test
    fun theStreamIsDemuxedLikeAFile() = TestMpv.withMpv("pause" to "yes") { mpv ->
        mpv.registerStreamProtocol(PROTOCOL, Provider(wavBytes(0.5)))
        test {
            load(mpv, "$PROTOCOL://tone")
            assertEquals("wav", mpv.getPropertyString("file-format"))
            assertEquals(0.5, mpv.getPropertyDouble("duration")!!, 0.05)
            val track = mpv.getPropertyNode("track-list").list!!.single()
            assertEquals("pcm_s16le", track["codec"]?.string)
            assertEquals(wavBytes(0.5).size.toLong(), mpv.getPropertyLong("stream-end"))
        }
    }

    @Test
    fun seeksWithinTheStream() = TestMpv.withMpv("pause" to "yes", "cache" to "no", "demuxer-max-bytes" to "128KiB") { mpv ->
        // far larger than what mpv buffers, so that it seeks the stream instead of reading on
        val provider = Provider(wavBytes(120.0))
        mpv.registerStreamProtocol(PROTOCOL, provider)
        test {
            load(mpv, "$PROTOCOL://long")
            // the restart that ends this seek, not the one that ends loading
            val restarted = async(start = CoroutineStart.UNDISPATCHED) {
                mpv.eventFlow()
                    .dropWhile { it != MpvPlayerEvent.Seek }
                    .first { it == MpvPlayerEvent.PlaybackRestart }
            }
            mpv.commandNode(MpvNode.command("seek", 100.0, "absolute+exact"))
            restarted.await()
            eventually { (mpv.getPropertyDouble("time-pos") ?: 0.0) >= 99.95 }
            assertEquals(100.0, mpv.getPropertyDouble("time-pos")!!, 0.05)
        }
        // the demuxer moved the stream to (just before) 100 s into the samples
        val target = Wav.HEADER_SIZE + (100.0 * SAMPLE_RATE * 2).toLong()
        val seeks = provider.streams.flatMap { s -> s.calls.filter { it.startsWith("seek ") } }
            .map { it.removePrefix("seek ").toLong() }
        assertTrue("seeks $seeks, expected one near $target", seeks.any { it in (target - 65536)..target })
    }

    @Test
    fun aProviderThatThrowsFailsTheLoad() = TestMpv.withMpv { mpv ->
        mpv.registerStreamProtocol(PROTOCOL) { uri -> throw IOException("no such item: $uri") }
        val end = endOf(mpv) { mpv.command(arrayOf("loadfile", "$PROTOCOL://missing")) }
        assertEquals(MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_ERROR, end.reason)
        assertEquals(MPVLib.MpvError.MPV_ERROR_LOADING_FAILED, end.error)
        // and the instance carries on
        assertTrue(mpv.getPropertyNode("mpv-version").string!!.startsWith("mpv "))
    }

    @Test
    fun aReadThatThrowsEndsTheFileAndTheStreamIsClosed() = TestMpv.withMpv { mpv ->
        // the header and a little audio, then the connection "breaks"
        val provider = Provider(wavBytes(2.0), failReadsAfter = 8000)
        mpv.registerStreamProtocol(PROTOCOL, provider)
        val end = endOf(mpv) { mpv.command(arrayOf("loadfile", "$PROTOCOL://breaks")) }
        assertTrue("ended: $end", end.reason == MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_EOF ||
            end.reason == MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_ERROR)
        for (stream in provider.streams) ApiTestSupport.await(stream.closed, "close")
        assertTrue(mpv.getPropertyNode("mpv-version").string!!.startsWith("mpv "))
    }

    @Test
    fun destroyingTheInstanceCancelsAndClosesAnOpenStream() {
        val mpv = TestMpv.create()
        val stalled = RecordingStream(ByteArray(0), blockReads = true)
        mpv.registerStreamProtocol(PROTOCOL) { stalled }
        try {
            mpv.command(arrayOf("loadfile", "$PROTOCOL://stalled"))
            ApiTestSupport.await(stalled.readBlocked, "the stalled read")
        } catch (t: Throwable) {
            mpv.destroy()
            throw t
        }
        // destroy() in another thread, so that a hang fails the test instead of the run
        val error = AtomicReference<Throwable?>()
        val destroyer = thread(name = "libmpv-test-destroy") {
            try {
                mpv.destroy()
            } catch (t: Throwable) {
                error.set(t)
            }
        }
        destroyer.join(TimeUnit.SECONDS.toMillis(TestMpv.TIMEOUT_SECONDS))
        if (destroyer.isAlive) fail("destroy() did not return with a stream open; stream calls: ${stalled.calls}")
        error.get()?.let { throw it }
        assertEquals("mpv cancelled the stalled read", 0L, stalled.cancelled.count)
        assertEquals("closed", 0L, stalled.closed.count)
        assertEquals(stalled.calls.toString(), 1, stalled.calls.count { it == "close" })
        assertFalse(stalled.calls.toString(), stalled.calls.any { it.startsWith("AFTER CLOSE") })
    }

    @Test
    fun aStreamCallbackCannotCallItsOwnInstance() = TestMpv.withMpv("pause" to "yes") { mpv ->
        val refused = AtomicReference<Throwable?>()
        mpv.registerStreamProtocol(PROTOCOL) {
            // mpv would deadlock here; the call throws instead
            try {
                mpv.getPropertyString("mpv-version")
            } catch (e: Throwable) {
                refused.set(e)
            }
            RecordingStream(wavBytes(0.25))
        }
        test { load(mpv, "$PROTOCOL://reentrant") }
        val e = refused.get()
        assertTrue("got $e", e is IllegalStateException)
        assertTrue(e!!.message!!, e.message!!.contains("stream callbacks"))
    }

    @Test
    fun protocolsCannotBeRegisteredTwiceOrShadowMpvs() = TestMpv.withMpv { mpv ->
        mpv.registerStreamProtocol(PROTOCOL) { RecordingStream(ByteArray(0)) }
        for (taken in listOf(PROTOCOL, "file", "http")) {
            val e = assertThrows(taken, MpvException::class.java) {
                mpv.registerStreamProtocol(taken) { RecordingStream(ByteArray(0)) }
            }
            assertEquals(MPVLib.MpvError.MPV_ERROR_INVALID_PARAMETER, e.error)
        }
    }

    @Test
    fun registeringAfterDestroyThrows() {
        val mpv = TestMpv.create()
        mpv.destroy()
        assertThrows(IllegalStateException::class.java) {
            mpv.registerStreamProtocol(PROTOCOL) { RecordingStream(ByteArray(0)) }
        }
    }

    @Test
    fun manyStreamsOneAfterAnother() = TestMpv.withMpv { mpv ->
        val provider = Provider(wavBytes(0.1))
        mpv.registerStreamProtocol(PROTOCOL, provider)
        repeat(5) { i ->
            val end = endOf(mpv) { mpv.command(arrayOf("loadfile", "$PROTOCOL://n$i")) }
            assertEquals(MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_EOF, end.reason)
        }
        test {
            withContext(Dispatchers.IO) {
                for (stream in provider.streams) ApiTestSupport.await(stream.closed, "close")
            }
        }
    }

    /**
     * libplayer attaches each of mpv's threads to the VM on its first callback and keeps it
     * attached: a thread attached per call would be a new java.lang.Thread every time. When
     * mpv's threads end (all of them by the time destroy() returns), they are detached, so
     * their Thread objects terminate.
     */
    @Test
    fun mpvsThreadsAreAttachedOnceAndDetachedWhenTheyEnd() {
        // 8 KB in reads of at most 256 bytes: dozens of callbacks
        val provider = Provider(wavBytes(0.5), maxRead = 256)
        val mpv = TestMpv.create()
        try {
            mpv.registerStreamProtocol(PROTOCOL, provider)
            val end = endOf(mpv) { mpv.command(arrayOf("loadfile", "$PROTOCOL://short-reads")) }
            assertEquals(MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_EOF, end.reason)
        } finally {
            mpv.destroy()
        }
        val reads = provider.streams.sumOf { s -> s.calls.count { it == "read" } }
        val threads = provider.streams.flatMap { it.threads }.toSet()
        assertTrue("$reads reads", reads >= 30)
        assertTrue("$reads reads on threads $threads", threads.size <= 4)
        test { eventually { threads.none { it.isAlive } } }
    }
}
