package dev.jdtech.mpv

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.jdtech.mpv.ApiTestSupport.load
import dev.jdtech.mpv.ApiTestSupport.test
import dev.jdtech.mpv.ApiTestSupport.withWavFile
import java.util.concurrent.CountDownLatch
import org.json.JSONArray
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Structured values (mpv_node): properties, commands and their results, and unobserveProperty. */
@RunWith(AndroidJUnit4::class)
class MPVLibNodeTest {

    @Test
    fun trackListIsStructured() = withWavFile("libmpv-test-node.wav", 0.5) { file ->
        TestMpv.withMpv("pause" to "yes") { mpv ->
            test { load(mpv, file.absolutePath) }

            val tracks = mpv.getPropertyNode("track-list")
            val list = tracks.list ?: throw AssertionError("track-list is not an array: $tracks")
            assertEquals("tracks: $tracks", 1, list.size)
            val track = list[0]
            assertEquals(1L, track["id"]?.long)
            assertEquals("audio", track["type"]?.string)
            assertEquals("pcm_s16le", track["codec"]?.string)
            assertEquals(true, track["selected"]?.boolean)
            assertEquals(8000L, track["demux-samplerate"]?.long)
            assertEquals(1L, track["demux-channel-count"]?.long)

            // the same data as the JSON the string getter returns, typed instead of text
            val json = JSONArray(mpv.getPropertyString("track-list")).getJSONObject(0)
            assertEquals(json.keys().asSequence().toSet(), track.map!!.keys)
            assertEquals(json.getLong("id"), track["id"]?.long)

            assertEquals(0.5, mpv.getPropertyNode("duration").double!!, 0.05)
            assertEquals(MpvNode.StringValue("wav"), mpv.getPropertyNode("file-format"))
        }
    }

    @Test
    fun commandsReturnStructuredResults() = TestMpv.withMpv { mpv ->
        // loadfile answers with a map holding the new entry's id
        val loaded = mpv.commandNode(MpvNode.command("loadfile", "/nonexistent/libmpv-test.wav", "append"))
        val id = loaded["playlist_entry_id"]?.long
        assertNotNull("loadfile result: $loaded", id)
        val playlist = mpv.getPropertyNode("playlist").list!!
        assertEquals(id, playlist.single()["id"]?.long)

        // named arguments, and a string result
        mpv.setPropertyNode("speed", MpvNode.DoubleValue(1.25))
        val expanded = mpv.commandNode(MpvNode.namedCommand("expand-text", mapOf("text" to "speed=\${speed}")))
        val text = expanded.string ?: throw AssertionError("expand-text result: $expanded")
        assertTrue(text, text.startsWith("speed="))
        assertEquals(text, 1.25, text.removePrefix("speed=").toDouble(), 0.0)

        // a command without a result
        assertEquals(MpvNode.None, mpv.commandNode(MpvNode.command("playlist-clear")))
    }

    @Test
    fun propertiesRoundTripAsNodes() = TestMpv.withMpv { mpv ->
        mpv.setPropertyNode("speed", MpvNode.DoubleValue(1.5))
        assertEquals(MpvNode.DoubleValue(1.5), mpv.getPropertyNode("speed"))
        mpv.setPropertyNode("mute", MpvNode.FlagValue(true))
        assertEquals(MpvNode.FlagValue(true), mpv.getPropertyNode("mute"))
        mpv.setPropertyNode("osd-duration", MpvNode.Int64Value(2345))
        assertEquals(2345, mpv.getPropertyInt("osd-duration"))
        val title = "notes 🎵 Ünïcödé"
        mpv.setPropertyNode("title", MpvNode.StringValue(title))
        assertEquals(title, mpv.getPropertyString("title"))

        // a map-typed option
        val opts = MpvNode.of(mapOf("libmpv-test-a" to "1", "libmpv-test-b" to "x y"))
        mpv.setPropertyNode("script-opts", opts)
        assertEquals(opts, mpv.getPropertyNode("script-opts"))
    }

    @Test
    fun mpvErrorsAreThrownWithTheirCode() = TestMpv.withMpv { mpv ->
        val notFound = assertThrows(MpvException::class.java) { mpv.getPropertyNode("libmpv-test-no-such-property") }
        assertEquals(MPVLib.MpvError.MPV_ERROR_PROPERTY_NOT_FOUND, notFound.error)
        val unavailable = assertThrows(MpvException::class.java) { mpv.getPropertyNode("duration") }
        assertEquals(MPVLib.MpvError.MPV_ERROR_PROPERTY_UNAVAILABLE, unavailable.error)
        assertTrue(unavailable.message!!.contains("property unavailable"))

        val badValue = assertThrows(MpvException::class.java) {
            mpv.setPropertyNode("speed", MpvNode.StringValue("fast"))
        }
        assertTrue("error ${badValue.error}", badValue.error < 0)
        val badCommand = assertThrows(MpvException::class.java) {
            mpv.commandNode(MpvNode.command("libmpv-test-no-such-command"))
        }
        assertTrue("error ${badCommand.error}", badCommand.error < 0)
        // and the instance is fine afterwards
        assertNotNull(mpv.getPropertyNode("mpv-version").string)
    }

    @Test
    fun unobservePropertyUndoesObserveProperty() = TestMpv.withMpv { mpv ->
        val volume = CountDownLatch(1)
        val speed = CountDownLatch(1)
        mpv.addObserver(object : MPVLib.EventObserver {
            override fun eventProperty(property: String, value: Double) {
                when (property) {
                    "volume" -> volume.countDown()
                    "speed" -> speed.countDown()
                }
            }
        })
        mpv.observeProperty("volume", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
        mpv.observeProperty("volume", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
        mpv.observeProperty("speed", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
        ApiTestSupport.await(volume, "the initial volume change")

        // mpv's own count of what it removed: both observations of volume, nothing else
        assertEquals(2, mpv.unobserveProperty("volume"))
        assertEquals(0, mpv.unobserveProperty("volume"))
        assertEquals(0, mpv.unobserveProperty("libmpv-test-never-observed"))
        // speed is still observed and delivered
        ApiTestSupport.await(speed, "the initial speed change")
        assertEquals(1, mpv.unobserveProperty("speed"))
    }

    @Test
    fun callsAfterDestroyThrow() {
        val mpv = TestMpv.create()
        mpv.destroy()
        assertThrows(IllegalStateException::class.java) { mpv.getPropertyNode("mpv-version") }
        assertThrows(IllegalStateException::class.java) { mpv.setPropertyNode("speed", MpvNode.DoubleValue(1.0)) }
        assertThrows(IllegalStateException::class.java) { mpv.commandNode(MpvNode.command("stop")) }
        assertThrows(IllegalStateException::class.java) { mpv.unobserveProperty("volume") }
    }
}
