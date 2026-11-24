package dev.jdtech.mpv.internal

import android.util.Log
import dev.jdtech.mpv.MPVLib.MpvError
import dev.jdtech.mpv.MpvStream
import dev.jdtech.mpv.MpvStreamProvider
import java.util.concurrent.atomic.AtomicBoolean

private const val TAG = "mpv"

/**
 * What libplayer's stream callbacks call for one registered protocol (`stream_jni.rs`):
 * the app's [MpvStreamProvider], with its failures turned into mpv's conventions. Called
 * on mpv's threads.
 */
internal class ProtocolAdapter(private val protocol: String, private val provider: MpvStreamProvider) {
    /** `open_fn`: null makes mpv fail the open with MPV_ERROR_LOADING_FAILED. */
    fun open(uri: String): StreamAdapter? =
        try {
            StreamAdapter(provider.open(uri))
        } catch (e: Exception) {
            Log.w(TAG, "$protocol: the stream provider could not open a stream", e)
            null
        }
}

/**
 * One open stream. mpv calls [read], [seek], [size] and [close] on one thread, in turn;
 * [cancel] on any thread, also during the others and after [close]. Each translates what
 * the [MpvStream] returns or throws into mpv's stream_cb conventions and never throws
 * itself (an Error aside, which libplayer logs and clears).
 */
internal class StreamAdapter(private val stream: MpvStream) {
    private val buffer = ByteArray(BUFFER_SIZE)
    private val closed = AtomicBoolean(false)

    /** The array [read] fills; libplayer copies from it into mpv's buffer. */
    fun buffer(): ByteArray = buffer

    /** `read_fn`: bytes read into [buffer] (1..[length]), 0 at the end, -1 on error. */
    fun read(length: Int): Int {
        if (closed.get() || length !in 1..buffer.size) return -1
        val n = try {
            stream.read(buffer, 0, length)
        } catch (e: Exception) {
            Log.w(TAG, "a stream's read failed", e)
            return -1
        }
        return when {
            n < 0 -> 0 // InputStream's -1 is mpv's 0
            n > length -> {
                Log.e(TAG, "a stream's read returned $n for $length bytes")
                -1
            }
            else -> n // 0 (nothing read) is the end for mpv as well
        }
    }

    /** `seek_fn`: the new position, MPV_ERROR_UNSUPPORTED, or MPV_ERROR_GENERIC. */
    fun seek(offset: Long): Long =
        try {
            val position = stream.seek(offset)
            if (position >= 0) position else MpvError.MPV_ERROR_GENERIC.toLong()
        } catch (e: UnsupportedOperationException) {
            MpvError.MPV_ERROR_UNSUPPORTED.toLong()
        } catch (e: Exception) {
            Log.w(TAG, "a stream's seek to $offset failed", e)
            MpvError.MPV_ERROR_GENERIC.toLong()
        }

    /** `size_fn`: the size, or MPV_ERROR_UNSUPPORTED if unknown. */
    fun size(): Long =
        try {
            stream.size().takeIf { it >= 0 } ?: MpvError.MPV_ERROR_UNSUPPORTED.toLong()
        } catch (e: Exception) {
            Log.w(TAG, "a stream's size failed", e)
            MpvError.MPV_ERROR_UNSUPPORTED.toLong()
        }

    /** `close_fn`: closes the stream once. */
    fun close() {
        if (!closed.compareAndSet(false, true)) return
        try {
            stream.close()
        } catch (e: Exception) {
            Log.w(TAG, "a stream's close failed", e)
        }
    }

    /** `cancel_fn`: passed on unless the stream is closed. Must not block. */
    fun cancel() {
        if (closed.get()) return
        try {
            stream.cancel()
        } catch (e: Exception) {
            Log.w(TAG, "a stream's cancel failed", e)
        }
    }

    companion object {
        /** The most one read hands mpv; mpv asks again for the rest. */
        const val BUFFER_SIZE = 128 * 1024
    }
}
