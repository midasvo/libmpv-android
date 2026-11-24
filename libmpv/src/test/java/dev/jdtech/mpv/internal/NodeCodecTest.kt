package dev.jdtech.mpv.internal

import dev.jdtech.mpv.MpvNode
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class NodeCodecTest {
    private companion object {
        /**
         * The same bytes as GOLDEN in libplayer's node.rs, which encodes them from a
         * hand-built mpv_node: the two sides agree on the format.
         */
        const val GOLDEN = "0700000001" +
            "0800000005" +
            "00000002 0069 0064 04 0000000000000001" +
            "00000004 0074 0079 0070 0065 01 00000005 0061 0075 0064 0069 006f" +
            "00000008 0073 0065 006c 0065 0063 0074 0065 0064 03 01" +
            "00000009 0064 0065 006d 0075 0078 002d 0066 0070 0073 05 3ff8000000000000" +
            "00000005 0074 0069 0074 006c 0065 01 00000002 00dc 0020"

        fun bytes(hex: String): ByteArray {
            val h = hex.replace(" ", "")
            return ByteArray(h.length / 2) { h.substring(2 * it, 2 * it + 2).toInt(16).toByte() }
        }

        val TRACK = MpvNode.ArrayValue(
            listOf(
                MpvNode.MapValue(
                    linkedMapOf(
                        "id" to MpvNode.Int64Value(1),
                        "type" to MpvNode.StringValue("audio"),
                        "selected" to MpvNode.FlagValue(true),
                        "demux-fps" to MpvNode.DoubleValue(1.5),
                        "title" to MpvNode.StringValue("Ü "),
                    ),
                ),
            ),
        )
    }

    @Test
    fun decodesWhatLibplayerEncodes() {
        val node = NodeCodec.decode(bytes(GOLDEN))
        assertEquals(TRACK, node)
        // and the order of the map's entries is mpv's
        assertEquals(listOf("id", "type", "selected", "demux-fps", "title"), node[0]!!.map!!.keys.toList())
    }

    @Test
    fun encodesWhatLibplayerDecodes() {
        assertArrayEquals(bytes(GOLDEN), NodeCodec.encode(TRACK))
    }

    @Test
    fun everyKindRoundTrips() {
        val node = MpvNode.MapValue(
            linkedMapOf(
                "none" to MpvNode.None,
                "false" to MpvNode.FlagValue(false),
                "min" to MpvNode.Int64Value(Long.MIN_VALUE),
                "max" to MpvNode.Int64Value(Long.MAX_VALUE),
                "nan" to MpvNode.DoubleValue(Double.NaN),
                "negative zero" to MpvNode.DoubleValue(-0.0),
                "infinity" to MpvNode.DoubleValue(Double.NEGATIVE_INFINITY),
                "empty" to MpvNode.StringValue(""),
                // a supplementary character, U+0000 and a lone surrogate: UTF-16 as is
                "text" to MpvNode.StringValue("notes 🎵 \u0000 \ud800 end"),
                "🎵 key" to MpvNode.ArrayValue(emptyList()),
                "map" to MpvNode.MapValue(emptyMap()),
                "bytes" to MpvNode.ByteArrayValue(byteArrayOf(0, -1, 127)),
                "no bytes" to MpvNode.ByteArrayValue(ByteArray(0)),
                "nested" to MpvNode.ArrayValue(listOf(MpvNode.ArrayValue(listOf(MpvNode.None)))),
            ),
        )
        val decoded = NodeCodec.decode(NodeCodec.encode(node))
        assertEquals(node, decoded)
        assertEquals((-0.0).toRawBits(), decoded["negative zero"]!!.double!!.toRawBits())
        assertTrue(decoded["nan"]!!.double!!.isNaN())
    }

    @Test
    fun scalarsHaveTheirTags() {
        assertArrayEquals(byteArrayOf(0), NodeCodec.encode(MpvNode.None))
        assertArrayEquals(byteArrayOf(3, 1), NodeCodec.encode(MpvNode.FlagValue(true)))
        assertArrayEquals(bytes("04 fffffffffffffffe"), NodeCodec.encode(MpvNode.Int64Value(-2)))
        assertArrayEquals(bytes("01 00000002 d83c dfb5"), NodeCodec.encode(MpvNode.StringValue("🎵")))
        assertArrayEquals(bytes("09 00000003 00 01 ff"), NodeCodec.encode(MpvNode.ByteArrayValue(byteArrayOf(0, 1, -1))))
    }

    @Test
    fun malformedBytesAreRefused() {
        for (bad in listOf(
            "", "02", "06", "03 02", "04 0000", "01 00000002 0061", "07 00000001",
            "07 ffffffff 00", "07 7fffffff 00", "08 00000001 00", "09 00000004 0102", "00 00", "0a",
        )) {
            assertThrows("'$bad'", IllegalArgumentException::class.java) { NodeCodec.decode(bytes(bad)) }
        }
    }

    @Test
    fun depthIsLimited() {
        var node: MpvNode = MpvNode.None
        repeat(NodeCodec.MAX_DEPTH) { node = MpvNode.ArrayValue(listOf(node)) }
        assertEquals(node, NodeCodec.decode(NodeCodec.encode(node)))
        val tooDeep = MpvNode.ArrayValue(listOf(node))
        assertThrows(IllegalArgumentException::class.java) { NodeCodec.encode(tooDeep) }
        val deepBytes = bytes("0700000001".repeat(NodeCodec.MAX_DEPTH + 1) + "00")
        assertThrows(IllegalArgumentException::class.java) { NodeCodec.decode(deepBytes) }
    }
}
