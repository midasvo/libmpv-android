package dev.jdtech.mpv

import java.io.File
import java.io.IOException
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.fail

/** Helpers for the tests of the node, async, flow and stream API. */
internal object ApiTestSupport {
    const val SAMPLE_RATE = 8000
    const val TIMEOUT_MS = TestMpv.TIMEOUT_SECONDS * 1000

    /** Runs [block] in a coroutine, failing it after the generous [TIMEOUT_MS]. */
    fun <T> test(block: suspend CoroutineScope.() -> T): T =
        runBlocking { withTimeout(TIMEOUT_MS) { block() } }

    /** A mono 8 kHz WAV of [seconds]. */
    fun wavBytes(seconds: Double): ByteArray = Wav.pcm16(SAMPLE_RATE, 1, (SAMPLE_RATE * seconds).toInt())

    fun <T> withWavFile(name: String, seconds: Double, block: (File) -> T): T {
        val file = File(TestMpv.cacheDir, name)
        try {
            file.writeBytes(wavBytes(seconds))
            return block(file)
        } finally {
            file.delete()
        }
    }

    /**
     * Loads [url] with `loadfile` (through [commandNode], whose result has the entry id) and
     * waits for FILE_LOADED; an END_FILE first fails the test. The event flow is collected
     * before loadfile is sent, so neither event can be missed.
     */
    suspend fun CoroutineScope.load(mpv: MPVLib, url: String): Long {
        val outcome = async(start = CoroutineStart.UNDISPATCHED) {
            mpv.eventFlow().first { it is MpvPlayerEvent.FileLoaded || it is MpvPlayerEvent.EndFile }
        }
        val entryId = mpv.commandNode(MpvNode.command("loadfile", url))["playlist_entry_id"]?.long
            ?: throw AssertionError("loadfile returned no playlist_entry_id")
        val event = outcome.await()
        if (event is MpvPlayerEvent.EndFile) fail("$url did not load: $event")
        return entryId
    }

    /** Waits (polling) until [condition] holds; the test's timeout bounds it. */
    suspend fun eventually(condition: () -> Boolean) {
        while (!condition()) delay(10)
    }

    fun await(latch: CountDownLatch, what: String) {
        if (!latch.await(TestMpv.TIMEOUT_SECONDS, TimeUnit.SECONDS)) fail("timed out waiting for $what")
    }
}

/**
 * An [MpvStream] over [data] that records every call. [blockReads] makes each read wait
 * until [cancel] (and records that it was blocked), as a stalled network read would.
 */
internal class RecordingStream(
    private val data: ByteArray,
    private val blockReads: Boolean = false,
    private val failReadsAfter: Int = Int.MAX_VALUE,
    private val maxRead: Int = Int.MAX_VALUE,
) : MpvStream {
    val calls: MutableList<String> = CopyOnWriteArrayList()
    val closed = CountDownLatch(1)
    val readBlocked = CountDownLatch(1)
    val cancelled = CountDownLatch(1)
    /** The Java threads the calls came on: mpv's own, attached by libplayer. */
    val threads: MutableSet<Thread> = java.util.Collections.synchronizedSet(mutableSetOf())

    @Volatile private var position = 0
    @Volatile private var isClosed = false

    private fun record(call: String) {
        if (isClosed) calls += "AFTER CLOSE: $call"
        calls += call
        threads += Thread.currentThread()
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        record("read")
        if (blockReads) {
            readBlocked.countDown()
            if (!cancelled.await(TestMpv.TIMEOUT_SECONDS * 2, TimeUnit.SECONDS)) throw IOException("never cancelled")
            throw IOException("cancelled")
        }
        if (position >= failReadsAfter) throw IOException("the connection broke")
        if (position >= data.size) return -1
        val n = minOf(length, maxRead, data.size - position)
        System.arraycopy(data, position, buffer, offset, n)
        position += n
        return n
    }

    override fun seek(position: Long): Long {
        record("seek $position")
        if (position > data.size) throw IOException("past the end")
        this.position = position.toInt()
        return position
    }

    override fun size(): Long {
        record("size")
        return data.size.toLong()
    }

    override fun cancel() {
        // may come after close (stream_cb.h allows it); the adapter filters that out
        calls += "cancel"
        cancelled.countDown()
    }

    override fun close() {
        record("close")
        isClosed = true
        closed.countDown()
    }
}
