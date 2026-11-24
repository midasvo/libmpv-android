package dev.jdtech.mpv.internal

import dev.jdtech.mpv.MPVLib.MpvError
import dev.jdtech.mpv.MpvStream
import java.io.ByteArrayInputStream
import java.io.IOException
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

/** The translation between an app's [MpvStream] and mpv's stream_cb conventions. */
class StreamAdapterTest {
    private class Recording(
        data: ByteArray,
        val seekable: Boolean = true,
        var readError: Exception? = null,
        val sizeError: Exception? = null,
    ) : MpvStream {
        private val input = ByteArrayInputStream(data)
        private val length = data.size.toLong()
        val calls = mutableListOf<String>()

        override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
            readError?.let { throw it }
            return input.read(buffer, offset, length)
        }

        override fun seek(position: Long): Long {
            if (!seekable) return super.seek(position)
            if (position > length) throw IOException("past the end")
            input.reset()
            input.skip(position)
            return position
        }

        override fun size(): Long = sizeError?.let { throw it } ?: length

        override fun cancel() {
            calls += "cancel"
        }

        override fun close() {
            calls += "close"
            throw IOException("close throws, and that is only logged")
        }
    }

    @Test
    fun readsIntoTheSharedBufferAndReportsTheEnd() {
        val data = ByteArray(300_000) { it.toByte() }
        val adapter = StreamAdapter(Recording(data))
        val buffer = adapter.buffer()
        assertEquals(StreamAdapter.BUFFER_SIZE, buffer.size)
        val out = java.io.ByteArrayOutputStream()
        while (true) {
            val n = adapter.read(buffer.size)
            if (n == 0) break
            out.write(buffer, 0, n)
        }
        assertArrayEquals(data, out.toByteArray())
        assertEquals("still at the end", 0, adapter.read(10))
    }

    @Test
    fun readFailuresAreMinusOne() {
        val stream = Recording(ByteArray(10), readError = IOException("network down"))
        val adapter = StreamAdapter(stream)
        assertEquals(-1, adapter.read(10))
        stream.readError = null
        assertEquals(-1, adapter.read(0)) // mpv never asks for 0 or more than the buffer
        assertEquals(-1, adapter.read(StreamAdapter.BUFFER_SIZE + 1))
        assertEquals(10, adapter.read(10))
    }

    @Test
    fun aStreamClaimingMoreThanAskedIsAnError() {
        val liar = object : MpvStream {
            override fun read(buffer: ByteArray, offset: Int, length: Int) = length + 1
            override fun close() {}
        }
        assertEquals(-1, StreamAdapter(liar).read(5))
    }

    @Test
    fun seekAndSize() {
        val adapter = StreamAdapter(Recording(ByteArray(100) { it.toByte() }))
        assertEquals(0L, adapter.seek(0))
        assertEquals(40L, adapter.seek(40))
        assertEquals(1, adapter.read(1))
        assertEquals(40.toByte(), adapter.buffer()[0])
        assertEquals(MpvError.MPV_ERROR_GENERIC.toLong(), adapter.seek(1000))
        assertEquals(100L, adapter.size())
    }

    @Test
    fun theDefaultsMeanNotSeekableAndUnknownSize() {
        val plain = object : MpvStream {
            override fun read(buffer: ByteArray, offset: Int, length: Int) = -1
            override fun close() {}
        }
        val adapter = StreamAdapter(plain)
        assertEquals(MpvError.MPV_ERROR_UNSUPPORTED.toLong(), adapter.seek(0))
        assertEquals(MpvError.MPV_ERROR_UNSUPPORTED.toLong(), adapter.size())
        adapter.cancel() // the default does nothing
        val failing = StreamAdapter(Recording(ByteArray(1), sizeError = IllegalStateException()))
        assertEquals(MpvError.MPV_ERROR_UNSUPPORTED.toLong(), failing.size())
        val negative = object : MpvStream {
            override fun read(buffer: ByteArray, offset: Int, length: Int) = -1
            override fun seek(position: Long) = -5L
            override fun close() {}
        }
        assertEquals(MpvError.MPV_ERROR_GENERIC.toLong(), StreamAdapter(negative).seek(0))
    }

    @Test
    fun closeHappensOnceAndEndsEverything() {
        val stream = Recording(ByteArray(10))
        val adapter = StreamAdapter(stream)
        adapter.cancel()
        adapter.close()
        adapter.close()
        adapter.cancel() // after close: not passed on
        assertEquals(listOf("cancel", "close"), stream.calls)
        assertEquals(-1, adapter.read(5))
    }

    @Test
    fun aProviderThatThrowsOpensNothing() {
        val refusing = ProtocolAdapter("test") { throw IOException("no such item") }
        assertNull(refusing.open("test://x"))
        val opening = ProtocolAdapter("test") { uri -> Recording(uri.toByteArray()) }
        assertNotNull(opening.open("test://x"))
    }
}
