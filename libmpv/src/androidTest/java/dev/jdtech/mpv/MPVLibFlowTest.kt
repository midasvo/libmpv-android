package dev.jdtech.mpv

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.jdtech.mpv.ApiTestSupport.eventually
import dev.jdtech.mpv.ApiTestSupport.load
import dev.jdtech.mpv.ApiTestSupport.test
import dev.jdtech.mpv.ApiTestSupport.withWavFile
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.filterIsInstance
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.takeWhile
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Property and event flows: observation lifetime, values, slow collectors, destroy(). */
@RunWith(AndroidJUnit4::class)
class MPVLibFlowTest {

    @Test
    fun aPropertyFlowObservesOnlyWhileCollected() = TestMpv.withMpv { mpv ->
        test {
            val unobservedBefore = mpv.runtime.unobserved.get()
            val values = Channel<Double?>(Channel.UNLIMITED)
            val job = launch(start = CoroutineStart.UNDISPATCHED) {
                mpv.doublePropertyFlow("volume").collect { values.send(it) }
            }
            assertEquals("the initial value", 100.0, values.receive())
            mpv.setPropertyDouble("volume", 42.0)
            assertEquals(42.0, values.receive())
            job.cancel()
            job.join()
            // mpv itself removed exactly one observation when the collector stopped
            assertEquals(1, mpv.runtime.unobserved.get() - unobservedBefore)

            // a new collection observes again, from the current value
            assertEquals(42.0, mpv.doublePropertyFlow("volume").first())
            assertEquals(2, mpv.runtime.unobserved.get() - unobservedBefore)
        }
    }

    @Test
    fun typedFlowsReadEachFormat() = TestMpv.withMpv { mpv ->
        test {
            mpv.setPropertyString("title", "libmpv flow 🎵")
            assertEquals("libmpv flow 🎵", mpv.stringPropertyFlow("title").first())
            mpv.setPropertyBoolean("mute", true)
            assertEquals(true, mpv.booleanPropertyFlow("mute").first())
            mpv.setPropertyInt("osd-duration", 1234)
            assertEquals(1234L, mpv.longPropertyFlow("osd-duration").first())
            assertEquals(MpvNode.FlagValue(true), mpv.nodePropertyFlow("mute").first())
            // unavailable: null, and None for nodes
            assertEquals(null, mpv.doublePropertyFlow("duration").first())
            assertEquals(MpvNode.None, mpv.nodePropertyFlow("duration").first())
            // the legacy observers do not see the flows' observations
        }
    }

    @Test
    fun theLegacyObserversDoNotSeeFlows() = TestMpv.withMpv { mpv ->
        val legacy = CopyOnWriteArrayList<String>()
        mpv.addObserver(object : MPVLib.EventObserver {
            override fun eventProperty(property: String) { legacy += property }
            override fun eventProperty(property: String, value: Double) { legacy += property }
        })
        test {
            assertEquals(100.0, mpv.doublePropertyFlow("volume").first())
            // a legacy observation after it, as a barrier: its first event comes after
            val speed = Channel<Unit>(1)
            mpv.addObserver(object : MPVLib.EventObserver {
                override fun eventProperty(property: String, value: Double) {
                    if (property == "speed") speed.trySend(Unit)
                }
            })
            mpv.observeProperty("speed", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
            speed.receive()
            assertEquals(listOf("speed"), legacy.toList())
        }
    }

    @Test
    fun trackListFlowFollowsLoading() = withWavFile("libmpv-test-flow.wav", 0.5) { file ->
        TestMpv.withMpv("pause" to "yes") { mpv ->
            test {
                val withAudio = async(start = CoroutineStart.UNDISPATCHED) {
                    mpv.nodePropertyFlow("track-list").first { tracks ->
                        tracks.list.orEmpty().any { it["type"]?.string == "audio" }
                    }
                }
                mpv.command(arrayOf("loadfile", file.absolutePath))
                val tracks = withAudio.await().list!!
                assertEquals("pcm_s16le", tracks.single()["codec"]?.string)
            }
        }
    }

    @Test
    fun eventsCarryMpvsDataThroughAPlayback() = withWavFile("libmpv-test-events.wav", 0.25) { file ->
        TestMpv.withMpv { mpv ->
            test {
                val events = async(start = CoroutineStart.UNDISPATCHED) {
                    mpv.eventFlow()
                        .takeWhile { it !is MpvPlayerEvent.EndFile }
                        .toList()
                }
                val end = async(start = CoroutineStart.UNDISPATCHED) {
                    mpv.eventFlow().filterIsInstance<MpvPlayerEvent.EndFile>().first()
                }
                val entryId = mpv.commandNode(MpvNode.command("loadfile", file.absolutePath))["playlist_entry_id"]!!.long!!
                val ended = end.await()
                assertEquals(
                    MpvPlayerEvent.EndFile(MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_EOF, 0, entryId, 0, 0),
                    ended,
                )
                val seen = events.await()
                assertEquals(MpvPlayerEvent.StartFile(entryId), seen.first { it is MpvPlayerEvent.StartFile })
                assertTrue("$seen", MpvPlayerEvent.FileLoaded in seen)
                assertTrue("$seen", seen.indexOf(MpvPlayerEvent.StartFile(entryId)) < seen.indexOf(MpvPlayerEvent.FileLoaded))
            }
        }
    }

    @Test
    fun aFailedLoadEndsWithItsError() = TestMpv.withMpv { mpv ->
        test {
            val end = async(start = CoroutineStart.UNDISPATCHED) {
                mpv.eventFlow().filterIsInstance<MpvPlayerEvent.EndFile>().first()
            }
            mpv.command(arrayOf("loadfile", "/nonexistent/libmpv-test.wav"))
            val e = end.await()
            assertEquals(MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_ERROR, e.reason)
            assertTrue("error ${e.error}", e.error < 0)
        }
    }

    @Test
    fun clientMessagesCarryTheirArguments() = TestMpv.withMpv { mpv ->
        test {
            val message = async(start = CoroutineStart.UNDISPATCHED) {
                mpv.eventFlow().filterIsInstance<MpvPlayerEvent.ClientMessage>().first()
            }
            mpv.commandNode(MpvNode.command("script-message", "libmpv-test", "Ünï 🎵"))
            assertEquals(listOf("libmpv-test", "Ünï 🎵"), message.await().args)
        }
    }

    /**
     * A collector that does not keep up must hold up neither mpv's event thread nor anyone
     * else: while it is stuck, an async call (whose reply the same event thread delivers)
     * and another collector still get through, and it later gets the latest value.
     */
    @Test
    fun aSlowPropertyCollectorDoesNotBlockMpv() = TestMpv.withMpv { mpv ->
        test {
            val gate = Channel<Unit>()
            val seen = CopyOnWriteArrayList<Double?>()
            val slow = launch(Dispatchers.Default, start = CoroutineStart.UNDISPATCHED) {
                mpv.doublePropertyFlow("speed").collect {
                    seen += it
                    gate.receive()
                }
            }
            eventually { seen.isNotEmpty() } // stuck on the first value
            // speed, not volume: volume stops at volume-max (130)
            for (i in 1..200) mpv.setPropertyDouble("speed", 1.0 + i / 100.0)
            assertEquals(3.0, mpv.doublePropertyFlow("speed").first { it == 3.0 }!!, 0.0)
            assertTrue(mpv.getPropertyAsync("mpv-version").string!!.startsWith("mpv "))
            // released, it skips to the latest value
            while (seen.lastOrNull() != 3.0) gate.send(Unit)
            assertTrue("$seen", seen.size < 200)
            slow.cancel()
        }
    }

    @Test
    fun aSlowEventCollectorIsToldWhatItMissed() = TestMpv.withMpv { mpv ->
        val count = 300
        test {
            val gate = Channel<Unit>()
            val slowSeen = CopyOnWriteArrayList<MpvPlayerEvent>()
            val slow = launch(Dispatchers.Default, start = CoroutineStart.UNDISPATCHED) {
                mpv.eventFlow().collect {
                    slowSeen += it
                    gate.receive()
                }
            }
            val fast = async(start = CoroutineStart.UNDISPATCHED) {
                mpv.eventFlow().filterIsInstance<MpvPlayerEvent.ClientMessage>()
                    .first { it.args == listOf("libmpv-test", "$count") }
            }
            for (i in 1..count) mpv.command(arrayOf("script-message", "libmpv-test", "$i"))
            fast.await() // all of them went through mpv's event thread meanwhile

            // drain the slow one up to the marker
            while (slowSeen.none { it is MpvPlayerEvent.Dropped }) gate.send(Unit)
            slow.cancel()
            val messages = slowSeen.filterIsInstance<MpvPlayerEvent.ClientMessage>().map { it.args[1].toInt() }
            val dropped = slowSeen.filterIsInstance<MpvPlayerEvent.Dropped>().single().count
            assertEquals("in order, none twice", (1..messages.size).toList(), messages)
            assertTrue("buffered $messages", messages.size <= 1 + 256)
            assertTrue("dropped $dropped of $count after ${messages.size}", messages.size + dropped >= count)
        }
    }

    @Test
    fun destroyCompletesTheFlowsAndLaterCollectionsThrow() {
        val mpv = TestMpv.create()
        test {
            val values = Channel<Double?>(Channel.UNLIMITED)
            val property = async(start = CoroutineStart.UNDISPATCHED) {
                mpv.doublePropertyFlow("volume").collect { values.send(it) }
                "completed"
            }
            val events = async(start = CoroutineStart.UNDISPATCHED) {
                mpv.eventFlow().collect { }
                "completed"
            }
            values.receive()
            withContext(Dispatchers.IO) { mpv.destroy() }
            assertEquals("completed", property.await())
            assertEquals("completed", events.await())
        }
        assertThrows(IllegalStateException::class.java) {
            kotlinx.coroutines.runBlocking { mpv.doublePropertyFlow("volume").first() }
        }
        assertThrows(IllegalStateException::class.java) {
            kotlinx.coroutines.runBlocking { mpv.eventFlow().first() }
        }
    }
}
