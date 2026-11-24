@file:JvmName("MpvStreams")

package dev.jdtech.mpv

import dev.jdtech.mpv.internal.Native
import dev.jdtech.mpv.internal.ProtocolAdapter
import java.io.IOException

/**
 * Opens the streams of a protocol registered with [registerStreamProtocol]: mpv calls
 * [open] for each URI with that protocol it is asked to play (`loadfile`, `sub-add`, ...).
 *
 * Threads: usually one of mpv's own threads, possibly for several URIs at once. But mpv
 * runs some synchronous commands on the thread that calls them, and when such a command
 * opens a URI itself (`load-config-file app://x`, say, through [MPVLib.command] or
 * [commandNode]), [open] and the stream's other methods run on that calling thread, in the
 * middle of the call: on the main thread if the call is made there. [open] may block (it
 * should still honour the timeouts the app wants, as nothing cancels an open).
 *
 * It must not call this [MPVLib] instance (mpv would deadlock: every such call, [MPVLib.destroy]
 * and [MPVLib.command] included, throws [IllegalStateException] instead), and must not wait
 * for a thread that is calling [MPVLib.destroy] (which waits for mpv to close its streams).
 */
fun interface MpvStreamProvider {
    /**
     * Opens [uri] (the whole URI, protocol included). Throw any [Exception] to refuse it:
     * mpv then fails to open the file with `MPV_ERROR_LOADING_FAILED` and the reason is
     * logged.
     */
    fun open(uri: String): MpvStream
}

/**
 * One stream mpv reads from, implemented by the app (for example over its own HTTP client,
 * or a `ContentResolver` input stream for a `content://` URI). Maps to the callbacks of
 * mpv's `mpv_stream_cb_info`.
 *
 * Threads: [read], [seek], [size] and [close] are called one at a time on the thread that
 * opened the stream (see [MpvStreamProvider]). [cancel] is called on a thread of
 * libplayer's own, "mpv-stream-cancel", also while one of the others is blocked; never on
 * the thread that cancels a coroutine or calls [MPVLib.destroy]. None of them may call this
 * [MPVLib] instance (see [MpvStreamProvider]). An exception thrown by any of them is logged
 * and becomes mpv's error for that call; it never reaches mpv's threads.
 *
 * Lifetime: mpv owns the stream from [MpvStreamProvider.open] until it calls [close],
 * exactly once, also when the file is stopped or the instance destroyed
 * ([MPVLib.destroy] returns only after every stream is closed). Once [close] has begun,
 * nothing is called again, [cancel] included: a [cancel] still running then is waited for
 * before [close] is called.
 */
interface MpvStream {
    /**
     * Reads up to [length] (at least 1) bytes into [buffer] from [offset], blocking until
     * at least one byte is available, like [java.io.InputStream.read]: returns the number
     * of bytes read, or -1 at the end of the stream. Throw (an [IOException], say) on an
     * error; mpv then treats the stream as ended. The [buffer] is only valid during the
     * call; do not keep it.
     */
    fun read(buffer: ByteArray, offset: Int, length: Int): Int

    /**
     * Moves to absolute byte [position] and returns it. mpv calls `seek(0)` right after
     * opening to learn whether the stream can seek, and decides for the stream's whole
     * life: if that first call throws (the default throws
     * [UnsupportedOperationException]) or returns a negative number, mpv never calls
     * [seek] again and reads the stream from start to end only. After a successful first
     * call, an exception fails just that one seek.
     */
    fun seek(position: Long): Long = throw UnsupportedOperationException("not seekable")

    /** The total size in bytes, or a negative number if it is not known (the default). */
    fun size(): Long = -1

    /**
     * Makes a [read] or [seek] that is blocked, or comes later, return soon (with an error
     * or the end of the stream). Called when mpv gives up on the stream: the file is
     * stopped, the command that opened it is aborted (a cancelled [commandAsync]), or the
     * instance is destroyed. It runs on libplayer's "mpv-stream-cancel" thread, one cancel
     * after another for all streams of all instances, shortly after mpv asked for it (also
     * after the [read] it was meant for has returned). Must not block: a blocked cancel
     * holds up the cancels of every other stream, and if a [read] waits for it,
     * [MPVLib.destroy] waits too. The default does nothing, which is fine only for streams
     * that never block for long.
     */
    fun cancel() {}

    /** Releases the stream. Called exactly once, last; must not block for long. */
    fun close()
}

/**
 * Registers [protocol] so that mpv opens `protocol://...` URIs through [provider] instead
 * of its own protocols: for example `registerStreamProtocol("app", ...)` and then
 * `loadfile app://item/42`. The registration lasts as long as the instance; it cannot be
 * removed or replaced. Call it before using the protocol, usually right after
 * [MPVLib.init]. Maps to `mpv_stream_cb_add_ro`.
 *
 * ```
 * // plays content:// URIs as "local://content://..." through the app's ContentResolver
 * mpv.registerStreamProtocol("local") { uri ->
 *     val input = contentResolver.openInputStream(Uri.parse(uri.removePrefix("local://")))
 *         ?: throw FileNotFoundException(uri)
 *     object : MpvStream {
 *         override fun read(buffer: ByteArray, offset: Int, length: Int) = input.read(buffer, offset, length)
 *         override fun close() = input.close()
 *     }
 * }
 * mpv.command(arrayOf("loadfile", "local://$contentUri"))
 * ```
 * (A protocol name FFmpeg or mpv already handle, such as `file`, `http` or, where FFmpeg
 * was built with it, `content`, cannot be registered.)
 *
 * The provider and every stream it opened are kept until mpv can no longer call them:
 * streams until their [MpvStream.close], the provider until [MPVLib.destroy] returns.
 *
 * @throws MpvException `MPV_ERROR_INVALID_PARAMETER` if the protocol is registered already
 *   or is one of mpv's own (such as `file` or `http`)
 * @throws IllegalStateException after [MPVLib.destroy]
 */
fun MPVLib.registerStreamProtocol(protocol: String, provider: MpvStreamProvider) {
    Native.addStreamProtocol(handle(), protocol, ProtocolAdapter(protocol, provider))
}
