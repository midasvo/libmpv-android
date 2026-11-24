package dev.jdtech.mpv.internal

import dev.jdtech.mpv.MpvNode
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.nio.BufferUnderflowException
import java.nio.ByteBuffer

/**
 * [MpvNode] to and from the bytes libplayer's `node.rs` reads and writes (its module
 * documentation has the format): one tag byte per node, mpv's own `mpv_format` number,
 * then the payload, big-endian; strings as UTF-16 code units.
 */
internal object NodeCodec {
    private const val NONE = 0
    private const val STRING = 1
    private const val FLAG = 3
    private const val INT64 = 4
    private const val DOUBLE = 5
    private const val ARRAY = 7
    private const val MAP = 8
    private const val BYTES = 9

    /** As in node.rs: deeper nodes are refused, on both sides. */
    const val MAX_DEPTH = 64

    fun encode(node: MpvNode): ByteArray {
        val bytes = ByteArrayOutputStream()
        DataOutputStream(bytes).use { write(it, node, 0) }
        return bytes.toByteArray()
    }

    private fun writeString(out: DataOutputStream, s: String) {
        out.writeInt(s.length)
        out.writeChars(s)
    }

    private fun write(out: DataOutputStream, node: MpvNode, depth: Int) {
        require(depth <= MAX_DEPTH) { "mpv node nested deeper than $MAX_DEPTH levels" }
        when (node) {
            MpvNode.None -> out.writeByte(NONE)
            is MpvNode.StringValue -> {
                out.writeByte(STRING)
                writeString(out, node.value)
            }
            is MpvNode.FlagValue -> {
                out.writeByte(FLAG)
                out.writeByte(if (node.value) 1 else 0)
            }
            is MpvNode.Int64Value -> {
                out.writeByte(INT64)
                out.writeLong(node.value)
            }
            is MpvNode.DoubleValue -> {
                out.writeByte(DOUBLE)
                out.writeLong(node.value.toRawBits())
            }
            is MpvNode.ArrayValue -> {
                out.writeByte(ARRAY)
                out.writeInt(node.values.size)
                for (v in node.values) write(out, v, depth + 1)
            }
            is MpvNode.MapValue -> {
                out.writeByte(MAP)
                out.writeInt(node.entries.size)
                for ((k, v) in node.entries) {
                    writeString(out, k)
                    write(out, v, depth + 1)
                }
            }
            is MpvNode.ByteArrayValue -> {
                out.writeByte(BYTES)
                out.writeInt(node.size)
                out.write(node.toByteArray())
            }
        }
    }

    /** @throws IllegalArgumentException if [bytes] is not exactly one encoded node */
    fun decode(bytes: ByteArray): MpvNode {
        val buf = ByteBuffer.wrap(bytes)
        val node = try {
            read(buf, 0)
        } catch (e: BufferUnderflowException) {
            throw IllegalArgumentException("truncated mpv node", e)
        }
        require(!buf.hasRemaining()) { "trailing bytes after an mpv node" }
        return node
    }

    private fun count(buf: ByteBuffer, minSize: Int): Int {
        val n = buf.getInt()
        // also refuses counts of 2^31 and more, which read as negative
        require(n >= 0 && n.toLong() * minSize <= buf.remaining()) { "bad count $n in an mpv node" }
        return n
    }

    private fun readString(buf: ByteBuffer): String {
        val n = count(buf, 2)
        val chars = CharArray(n)
        buf.asCharBuffer().get(chars)
        buf.position(buf.position() + 2 * n)
        return String(chars)
    }

    private fun read(buf: ByteBuffer, depth: Int): MpvNode {
        require(depth <= MAX_DEPTH) { "mpv node nested deeper than $MAX_DEPTH levels" }
        return when (val tag = buf.get().toInt()) {
            NONE -> MpvNode.None
            STRING -> MpvNode.StringValue(readString(buf))
            FLAG -> when (buf.get().toInt()) {
                0 -> MpvNode.FlagValue(false)
                1 -> MpvNode.FlagValue(true)
                else -> throw IllegalArgumentException("bad flag in an mpv node")
            }
            INT64 -> MpvNode.Int64Value(buf.getLong())
            DOUBLE -> MpvNode.DoubleValue(Double.fromBits(buf.getLong()))
            ARRAY -> {
                val n = count(buf, 1)
                MpvNode.ArrayValue(List(n) { read(buf, depth + 1) })
            }
            MAP -> {
                val n = count(buf, 5)
                val entries = LinkedHashMap<String, MpvNode>(n * 4 / 3 + 1)
                repeat(n) {
                    val key = readString(buf)
                    entries[key] = read(buf, depth + 1)
                }
                MpvNode.MapValue(entries)
            }
            BYTES -> {
                val n = count(buf, 1)
                val data = ByteArray(n)
                buf.get(data)
                MpvNode.ByteArrayValue(data)
            }
            else -> throw IllegalArgumentException("unknown tag $tag in an mpv node")
        }
    }
}
