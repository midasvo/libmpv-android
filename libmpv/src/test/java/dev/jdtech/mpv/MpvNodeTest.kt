package dev.jdtech.mpv

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class MpvNodeTest {
    @Test
    fun plainValuesBecomeNodes() {
        assertEquals(MpvNode.None, MpvNode.of(null))
        assertEquals(MpvNode.StringValue("x"), MpvNode.of("x"))
        assertEquals(MpvNode.FlagValue(true), MpvNode.of(true))
        assertEquals(MpvNode.Int64Value(3), MpvNode.of(3))
        assertEquals(MpvNode.Int64Value(3), MpvNode.of(3L))
        assertEquals(MpvNode.Int64Value(3), MpvNode.of(3.toShort()))
        assertEquals(MpvNode.DoubleValue(0.5), MpvNode.of(0.5f))
        assertEquals(MpvNode.ByteArrayValue(byteArrayOf(1)), MpvNode.of(byteArrayOf(1)))
        assertEquals(
            MpvNode.ArrayValue(listOf(MpvNode.Int64Value(1), MpvNode.None)),
            MpvNode.of(listOf(1, null)),
        )
        assertEquals(MpvNode.of(listOf("a")), MpvNode.of(arrayOf("a")))
        assertEquals(
            MpvNode.MapValue(mapOf("k" to MpvNode.ArrayValue(listOf(MpvNode.FlagValue(false))))),
            MpvNode.of(mapOf("k" to setOf(false))),
        )
        val node = MpvNode.Int64Value(9)
        assertTrue(MpvNode.of(node) === node)
        assertThrows(IllegalArgumentException::class.java) { MpvNode.of(Any()) }
        assertThrows(IllegalArgumentException::class.java) { MpvNode.of(mapOf(1 to 2)) }
    }

    @Test
    fun accessorsReadPaths() {
        val tracks = MpvNode.of(
            listOf(
                mapOf("id" to 1, "type" to "video", "selected" to false),
                mapOf("id" to 2, "type" to "audio", "selected" to true, "demux-samplerate" to 8000),
            ),
        )
        val audio = tracks.list!!.single { it["type"]?.string == "audio" }
        assertEquals(2L, audio["id"]?.long)
        assertEquals(true, audio["selected"]?.boolean)
        assertEquals(8000.0, audio["demux-samplerate"]?.double)
        assertNull(audio["missing"])
        assertNull(tracks["id"])
        assertNull(tracks[5])
        assertNull(MpvNode.DoubleValue(1.5).long)
        assertNull(MpvNode.StringValue("1").long)
        assertTrue(MpvNode.None.isNone)
        assertFalse(MpvNode.FlagValue(false).isNone)
    }

    @Test
    fun byteArraysAreCopiedAndComparedByContent() {
        val data = byteArrayOf(1, 2, 3)
        val node = MpvNode.ByteArrayValue(data)
        data[0] = 9
        assertArrayEquals(byteArrayOf(1, 2, 3), node.bytes)
        node.bytes!![1] = 9
        assertArrayEquals(byteArrayOf(1, 2, 3), node.toByteArray())
        assertEquals(MpvNode.ByteArrayValue(byteArrayOf(1, 2, 3)), node)
        assertEquals(MpvNode.ByteArrayValue(byteArrayOf(1, 2, 3)).hashCode(), node.hashCode())
        assertNotEquals(MpvNode.ByteArrayValue(byteArrayOf(1, 2)), node)
        assertEquals(3, node.size)
    }

    @Test
    fun commandBuilders() {
        assertEquals(
            MpvNode.ArrayValue(
                listOf(MpvNode.StringValue("seek"), MpvNode.DoubleValue(1.5), MpvNode.StringValue("absolute")),
            ),
            MpvNode.command("seek", 1.5, "absolute"),
        )
        val named = MpvNode.namedCommand("loadfile", mapOf("url" to "a.wav", "flags" to "append"))
        assertEquals(listOf("_name", "url", "flags"), named.map!!.keys.toList())
        assertEquals("loadfile", named["_name"]?.string)
    }

    @Test
    fun exceptionsCarryMpvsCodeAndText() {
        val e = MpvException(MPVLib.MpvError.MPV_ERROR_PROPERTY_UNAVAILABLE, "getPropertyNode(duration)")
        assertEquals(-10, e.error)
        assertEquals("getPropertyNode(duration) failed: property unavailable (-10)", e.message)
        assertEquals("x failed: unknown error (-99)", MpvException(-99, "x").message)
    }
}
