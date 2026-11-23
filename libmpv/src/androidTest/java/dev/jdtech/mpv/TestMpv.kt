package dev.jdtech.mpv

import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.PI
import kotlin.math.roundToInt
import kotlin.math.sin
import org.junit.Assert.assertEquals

/** Shared helpers for the instrumented tests that need a real, playing [MPVLib]. */
internal object TestMpv {
    /**
     * Generous: CI runs these on a software-rendered x86_64 emulator, where
     * opening a file can take seconds. A passing test never waits this long.
     */
    const val TIMEOUT_SECONDS = 20L

    val cacheDir: File
        get() = InstrumentationRegistry.getInstrumentation().targetContext.cacheDir

    /**
     * Creates and initializes an instance with `vo=null` and `ao=null`, plus [options]
     * (set before init, in order). The caller must [MPVLib.destroy] it.
     */
    fun create(vararg options: Pair<String, String>): MPVLib {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val mpv = MPVLib.create(context) ?: throw AssertionError("MPVLib.create returned null")
        try {
            for ((name, value) in listOf("vo" to "null", "ao" to "null") + options)
                assertEquals("setOptionString($name, $value)", 0, mpv.setOptionString(name, value))
            mpv.init()
        } catch (t: Throwable) {
            mpv.destroy()
            throw t
        }
        return mpv
    }

    /** Runs [block] with a fresh instance from [create] and always destroys it. */
    inline fun <T> withMpv(vararg options: Pair<String, String>, block: (MPVLib) -> T): T {
        val mpv = create(*options)
        try {
            return block(mpv)
        } finally {
            mpv.destroy()
        }
    }

    fun await(latch: CountDownLatch): Boolean = latch.await(TIMEOUT_SECONDS, TimeUnit.SECONDS)

    /**
     * Splits an mpv string list read with `getPropertyString`: unlike node-typed
     * properties (JSON), string lists come back joined with ','.
     */
    fun splitList(joined: String): List<String> =
        if (joined.isEmpty()) emptyList() else joined.split(',')
}

/**
 * Builds canonical PCM WAV files: a 44-byte header (RIFF/WAVE, one `fmt ` chunk
 * with format tag 1, one `data` chunk, all fields little-endian), then the samples.
 * Free of Android classes so the byte layout can be checked on a plain JVM.
 */
internal object Wav {
    const val HEADER_SIZE = 44

    /**
     * 16-bit signed PCM with [channels] interleaved channels and [frames] sample
     * frames of a quiet 440 Hz sine: the duration is exactly frames / sampleRate seconds.
     */
    @JvmStatic
    fun pcm16(sampleRate: Int, channels: Int, frames: Int): ByteArray {
        require(sampleRate > 0 && channels > 0 && frames >= 0)
        val blockAlign = channels * 2
        val dataSize = frames * blockAlign
        val buf = ByteBuffer.allocate(HEADER_SIZE + dataSize).order(ByteOrder.LITTLE_ENDIAN)

        buf.put("RIFF".toByteArray(Charsets.US_ASCII))
        buf.putInt(36 + dataSize) // RIFF chunk size: the whole file minus these first 8 bytes
        buf.put("WAVE".toByteArray(Charsets.US_ASCII))

        buf.put("fmt ".toByteArray(Charsets.US_ASCII))
        buf.putInt(16) // fmt chunk size for PCM
        buf.putShort(1) // WAVE_FORMAT_PCM
        buf.putShort(channels.toShort())
        buf.putInt(sampleRate)
        buf.putInt(sampleRate * blockAlign) // byte rate
        buf.putShort(blockAlign.toShort())
        buf.putShort(16) // bits per sample

        buf.put("data".toByteArray(Charsets.US_ASCII))
        buf.putInt(dataSize)
        for (i in 0 until frames) {
            val sample = (sin(2.0 * PI * 440.0 * i / sampleRate) * 0.25 * Short.MAX_VALUE)
                .roundToInt().toShort()
            repeat(channels) { buf.putShort(sample) }
        }
        return buf.array()
    }

    fun write(file: File, sampleRate: Int, channels: Int, frames: Int): File {
        file.writeBytes(pcm16(sampleRate, channels, frames))
        return file
    }
}
