package dev.jdtech.mpv.r8test

import android.content.Context
import dev.jdtech.mpv.MPVLib
import dev.jdtech.mpv.MpvException
import dev.jdtech.mpv.MpvNode
import dev.jdtech.mpv.MpvPlayerEvent
import dev.jdtech.mpv.MpvStream
import dev.jdtech.mpv.commandAsync
import dev.jdtech.mpv.doublePropertyFlow
import dev.jdtech.mpv.eventFlow
import dev.jdtech.mpv.getDoublePropertyAsync
import dev.jdtech.mpv.getPropertyAsync
import dev.jdtech.mpv.getPropertyNode
import dev.jdtech.mpv.registerStreamProtocol
import dev.jdtech.mpv.setLogcatLevel
import dev.jdtech.mpv.setPropertyNode
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.filterIsInstance
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout

/**
 * Uses the library in the minified app, on every path where libplayer.so calls into Kotlin
 * by name (JNI `FindClass`/`GetMethodID` and the `native` methods' symbol names): instance
 * creation (which looks up every callback, the stream adapters and MpvException), the log and
 * event callbacks, node values, an mpv error thrown as MpvException, async replies, a property
 * flow and the event flow, and a custom stream protocol (open, read, seek, size, close, and
 * cancel). Kept by the app's own rule only, so the test can call it; nothing else of the
 * library is kept but by the library's consumer rules.
 */
object Probe {
    private const val TIMEOUT_SECONDS = 20L
    private const val PROTOCOL = "r8test"

    /** The steps [run] goes through, in order. */
    val STEPS = listOf(
        "create",
        "log callback",
        "event callback",
        "node property",
        "MpvException",
        "async",
        "flow",
        "stream protocol",
        "stream cancel",
        "close",
    )

    /** Goes through [STEPS] and returns the names of those that passed; throws at the first failure. */
    fun run(context: Context): List<String> {
        val done = mutableListOf<String>()
        fun step(name: String, block: () -> Unit) {
            try {
                block()
            } catch (t: Throwable) {
                throw AssertionError("step '$name' failed (after $done): $t", t)
            }
            done += name
        }

        val mpv = MPVLib.create(context) ?: throw AssertionError("MPVLib.create returned null")
        mpv.use {
            step("create") {
                check(mpv.setOptionString("vo", "null") == 0)
                check(mpv.setOptionString("ao", "null") == 0)
                mpv.init()
                mpv.setLogcatLevel(MPVLib.MpvLogLevel.MPV_LOG_LEVEL_WARN)
            }
            step("log callback") {
                val marker = "r8test-${System.nanoTime()}"
                val seen = CountDownLatch(1)
                mpv.addLogObserver(object : MPVLib.LogObserver {
                    override fun logMessage(prefix: String, level: Int, text: String) {
                        if (text.contains(marker)) seen.countDown()
                    }
                })
                mpv.command(arrayOf("print-text", marker))
                await(seen, "the log message")
            }
            step("event callback") {
                val volume = CountDownLatch(1)
                val events = CountDownLatch(1)
                mpv.addObserver(object : MPVLib.EventObserver {
                    override fun eventProperty(property: String, value: Double) {
                        if (property == "volume") volume.countDown()
                    }

                    override fun event(eventId: Int) {
                        if (eventId == MPVLib.MpvEvent.MPV_EVENT_CLIENT_MESSAGE) events.countDown()
                    }
                })
                mpv.observeProperty("volume", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
                mpv.command(arrayOf("script-message", "r8test"))
                await(volume, "eventProperty")
                await(events, "event")
            }
            step("node property") {
                mpv.setPropertyNode("speed", MpvNode.DoubleValue(1.25))
                check(mpv.getPropertyNode("speed") == MpvNode.DoubleValue(1.25))
                check(mpv.getPropertyNode("mpv-version").string!!.startsWith("mpv "))
            }
            step("MpvException") {
                try {
                    mpv.getPropertyNode("duration")
                    throw AssertionError("no MpvException")
                } catch (e: MpvException) {
                    check(e.error == MPVLib.MpvError.MPV_ERROR_PROPERTY_UNAVAILABLE) { "error ${e.error}" }
                }
            }
            step("async") {
                coroutine {
                    check(mpv.getPropertyAsync("mpv-version").string!!.startsWith("mpv "))
                    check(mpv.commandAsync(MpvNode.command("expand-text", "x")) == MpvNode.StringValue("x"))
                    check(mpv.getDoublePropertyAsync("speed") == 1.25)
                }
            }
            step("flow") {
                coroutine {
                    mpv.setPropertyNode("volume", MpvNode.DoubleValue(42.0))
                    check(mpv.doublePropertyFlow("volume").first() == 42.0)
                    val message = async(start = CoroutineStart.UNDISPATCHED) {
                        mpv.eventFlow().filterIsInstance<MpvPlayerEvent.ClientMessage>().first()
                    }
                    mpv.command(arrayOf("script-message", "r8test", "flow"))
                    check(message.await().args == listOf("r8test", "flow"))
                }
            }
            step("stream protocol") {
                val streams = CopyOnWriteArrayList<MemoryStream>()
                mpv.registerStreamProtocol(PROTOCOL) { MemoryStream(wav(0.25)).also { streams += it } }
                coroutine {
                    val end = async(start = CoroutineStart.UNDISPATCHED) {
                        mpv.eventFlow().filterIsInstance<MpvPlayerEvent.EndFile>().first()
                    }
                    mpv.command(arrayOf("loadfile", "$PROTOCOL://tone"))
                    val e = end.await()
                    check(e.reason == MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_EOF) { "$e" }
                }
                check(streams.isNotEmpty())
                for (s in streams) await(s.closed, "the stream's close")
                check(streams.any { it.reads > 1 }) { "read ${streams.map { it.reads }}" }
            }
        }
        // MPVLib.use called close(): destroyed
        step("stream cancel") {
            try {
                mpv.getPropertyString("mpv-version")
                throw AssertionError("not destroyed by close()")
            } catch (e: IllegalStateException) {
                // expected
            }
            // a second instance, destroyed while a read blocks: mpv cancels the stream
            val other = MPVLib.create(context) ?: throw AssertionError("MPVLib.create returned null")
            val stalled = MemoryStream(ByteArray(0), blockReads = true)
            other.use {
                check(other.setOptionString("vo", "null") == 0)
                check(other.setOptionString("ao", "null") == 0)
                other.init()
                other.registerStreamProtocol(PROTOCOL) { stalled }
                other.command(arrayOf("loadfile", "$PROTOCOL://stalled"))
                await(stalled.readBlocked, "the stalled read")
            }
            await(stalled.cancelled, "the stream's cancel")
            await(stalled.closed, "the stream's close")
        }
        step("close") { mpv.close() }
        return done
    }

    private fun await(latch: CountDownLatch, what: String) {
        if (!latch.await(TIMEOUT_SECONDS, TimeUnit.SECONDS)) throw AssertionError("timed out waiting for $what")
    }

    private fun coroutine(block: suspend kotlinx.coroutines.CoroutineScope.() -> Unit) =
        runBlocking { withTimeout(TIMEOUT_SECONDS * 1000) { block() } }

    /** A WAV of [seconds] of silence: mono, 8 kHz, 16-bit PCM. */
    private fun wav(seconds: Double): ByteArray {
        val dataSize = (8000 * seconds).toInt() * 2
        return ByteBuffer.allocate(44 + dataSize).order(ByteOrder.LITTLE_ENDIAN).apply {
            put("RIFF".toByteArray()); putInt(36 + dataSize); put("WAVE".toByteArray())
            put("fmt ".toByteArray()); putInt(16); putShort(1); putShort(1); putInt(8000); putInt(16000)
            putShort(2); putShort(16)
            put("data".toByteArray()); putInt(dataSize)
        }.array()
    }

    /** A seekable stream over [data]; with [blockReads], each read waits for [cancel]. */
    private class MemoryStream(private val data: ByteArray, private val blockReads: Boolean = false) : MpvStream {
        val readBlocked = CountDownLatch(1)
        val cancelled = CountDownLatch(1)
        val closed = CountDownLatch(1)

        @Volatile var reads = 0
        @Volatile private var position = 0

        override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
            reads++
            if (blockReads) {
                readBlocked.countDown()
                cancelled.await(TIMEOUT_SECONDS * 2, TimeUnit.SECONDS)
                throw IOException("cancelled")
            }
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

        override fun cancel() {
            cancelled.countDown()
        }

        override fun close() {
            closed.countDown()
        }
    }
}
