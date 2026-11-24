package dev.jdtech.mpv

import android.os.Process
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.jdtech.mpv.ApiTestSupport.eventually
import dev.jdtech.mpv.ApiTestSupport.load
import dev.jdtech.mpv.ApiTestSupport.test
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONArray
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Real playback through the demuxer and a decoder (`vo=null`, `ao=null`), and the
 * typed property accessors. Every test creates and destroys its own [MPVLib].
 */
@RunWith(AndroidJUnit4::class)
class MPVLibPlaybackTest {

    private companion object {
        const val SAMPLE_RATE = 8000
        const val NO_SUCH_PROPERTY = "libmpv-test-no-such-property"
    }

    /** What was read while the file was loaded and paused, before it played to the end. */
    private class Loaded(val path: String?, val filename: String?, val duration: Double?)

    /**
     * Loads [file] paused, checks it was demuxed as a WAV with one PCM audio track
     * and the expected [duration], unpauses, and requires a clean EOF (reason EOF,
     * error 0), which needs the PCM decoder and the audio chain to have run.
     */
    private fun playWavToEnd(file: File, duration: Double): Loaded = TestMpv.withMpv("pause" to "yes") { mpv ->
        val loadedOrEnded = CountDownLatch(1)
        val ended = CountDownLatch(1)
        val loaded = AtomicBoolean(false)
        val endFiles = AtomicInteger(0)
        val endReason = AtomicInteger(-1)
        val endError = AtomicInteger(Int.MIN_VALUE)

        mpv.addObserver(object : MPVLib.EventObserver {
            override fun event(eventId: Int) {
                if (eventId == MPVLib.MpvEvent.MPV_EVENT_FILE_LOADED) {
                    loaded.set(true)
                    loadedOrEnded.countDown()
                }
            }

            // event(MPV_EVENT_END_FILE) follows this one; only this callback is counted.
            override fun eventEndFile(reason: Int, error: Int) {
                endReason.set(reason)
                endError.set(error)
                endFiles.incrementAndGet()
                ended.countDown()
                loadedOrEnded.countDown()
            }
        })

        assertEquals("loadfile was rejected", 0, mpv.commandWithResult(arrayOf("loadfile", file.absolutePath)))
        assertTrue("neither FILE_LOADED nor END_FILE for ${describe(file.name)}", TestMpv.await(loadedOrEnded))
        assertTrue(
            "${describe(file.name)} did not load: end-file reason ${endReason.get()}, error ${endError.get()}",
            loaded.get(),
        )

        val info = Loaded(
            path = mpv.getPropertyString("path"),
            filename = mpv.getPropertyString("filename"),
            duration = mpv.getPropertyDouble("duration"),
        )
        val trackList = mpv.getPropertyString("track-list")
        assertEquals("file-format", "wav", mpv.getPropertyString("file-format"))
        val actualDuration = info.duration ?: throw AssertionError("duration unavailable after FILE_LOADED")
        assertEquals("duration", duration, actualDuration, 0.05)
        assertTrue("track-list: $trackList", trackList != null)
        val tracks = JSONArray(trackList)
        assertEquals("track-list: $trackList", 1, tracks.length())
        val track = tracks.getJSONObject(0)
        assertEquals("track-list: $trackList", "audio", track.optString("type"))
        assertEquals("track-list: $trackList", "pcm_s16le", track.optString("codec"))
        assertTrue("audio track not selected, track-list: $trackList", track.optBoolean("selected"))

        mpv.setPropertyBoolean("pause", false)
        assertTrue("no end-file after unpausing", TestMpv.await(ended))
        assertEquals(
            "end-file reason (error was ${endError.get()})",
            MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_EOF, endReason.get(),
        )
        assertEquals("end-file error", 0, endError.get())
        assertEquals("end-file count", 1, endFiles.get())
        info
    }

    private fun withWav(name: String, frames: Int, block: (File) -> Unit) {
        val file = File(TestMpv.cacheDir, name)
        try {
            Wav.write(file, SAMPLE_RATE, 1, frames)
            block(file)
        } finally {
            file.delete()
        }
    }

    @Test
    fun playsGeneratedWavToEof() {
        // 0.5 s: long enough to be a real playback, short enough for CI
        withWav("libmpv-test-tone.wav", SAMPLE_RATE / 2) { file ->
            val info = playWavToEnd(file, 0.5)
            assertEquals(file.absolutePath, info.path)
            assertEquals(file.name, info.filename)
        }
    }

    @Test
    fun playsWavWithNonAsciiFileName() {
        // Basic Multilingual Plane only: Latin-1, CJK, a dash and a check mark
        val name = "libmpv-test Ünïcödé – 日本語 ✓.wav"
        withWav(name, SAMPLE_RATE / 4) { file ->
            val info = playWavToEnd(file, 0.25)
            assertEquals("path ${describe(info.path)}", file.absolutePath, info.path)
            assertEquals("filename ${describe(info.filename)}", name, info.filename)
        }
    }

    @Test
    fun playsWavWithSupplementaryPlaneFileName() {
        // U+1F3B5 needs a surrogate pair in Java and four bytes in UTF-8; JNI's
        // Modified UTF-8 encodes it differently, so this checks both directions.
        val name = "libmpv-test ëmoji 🎵.wav"
        withWav(name, SAMPLE_RATE / 4) { file ->
            val info = playWavToEnd(file, 0.25)
            assertEquals("path ${describe(info.path)}", file.absolutePath, info.path)
            assertEquals("filename ${describe(info.filename)}", name, info.filename)
        }
    }

    @Test
    fun typedPropertiesRoundTrip() = TestMpv.withMpv { mpv ->
        mpv.setPropertyDouble("speed", 1.75)
        assertEquals(1.75, mpv.getPropertyDouble("speed")!!, 0.0)

        mpv.setPropertyBoolean("mute", true)
        assertEquals(true, mpv.getPropertyBoolean("mute"))
        mpv.setPropertyBoolean("mute", false)
        assertEquals(false, mpv.getPropertyBoolean("mute"))

        mpv.setPropertyInt("osd-duration", 2345)
        assertEquals(2345, mpv.getPropertyInt("osd-duration"))
        assertEquals(2345L, mpv.getPropertyLong("osd-duration"))

        // getPropertyLong must not truncate. setPropertyInt takes an Int, so set it as a string.
        // The option's maximum is SIZE_MAX / 2, which is below 2^32 on 32-bit ABIs.
        val big = if (Process.is64Bit()) 6_000_000_000L else 2_000_000_000L
        mpv.setPropertyString("demuxer-max-bytes", big.toString())
        assertEquals(big, mpv.getPropertyLong("demuxer-max-bytes"))

        mpv.setPropertyString("title", "libmpv test title")
        assertEquals("libmpv test title", mpv.getPropertyString("title"))
    }

    @Test
    fun nonAsciiStringPropertyRoundTrips() = TestMpv.withMpv { mpv ->
        val value = "Ünïcödé – 日本語 ✓ Ωmega"
        mpv.setPropertyString("title", value)
        val actual = mpv.getPropertyString("title")
        assertEquals("title read back as ${describe(actual)}", value, actual)
    }

    @Test
    fun supplementaryPlaneStringPropertyRoundTrips() = TestMpv.withMpv { mpv ->
        // U+1F3B5 MUSICAL NOTE and U+1D11E MUSICAL SYMBOL G CLEF
        val value = "notes 🎵 𝄞 end"
        mpv.setPropertyString("title", value)
        val actual = mpv.getPropertyString("title")
        assertEquals("title read back as ${describe(actual)}", value, actual)
    }

    @Test
    fun missingPropertiesReadAsNull() = TestMpv.withMpv { mpv ->
        assertNull(mpv.getPropertyString(NO_SUCH_PROPERTY))
        assertNull(mpv.getPropertyInt(NO_SUCH_PROPERTY))
        assertNull(mpv.getPropertyLong(NO_SUCH_PROPERTY))
        assertNull(mpv.getPropertyDouble(NO_SUCH_PROPERTY))
        assertNull(mpv.getPropertyBoolean(NO_SUCH_PROPERTY))

        // Setting one only logs an error.
        mpv.setPropertyString(NO_SUCH_PROPERTY, "x")
        mpv.setPropertyInt(NO_SUCH_PROPERTY, 1)
        mpv.setPropertyDouble(NO_SUCH_PROPERTY, 1.0)
        mpv.setPropertyBoolean(NO_SUCH_PROPERTY, true)

        // Existing properties that are unavailable while nothing is loaded
        assertNull(mpv.getPropertyDouble("duration"))
        assertNull(mpv.getPropertyString("path"))
    }

    private class SubtitleCase(val extension: String, val codec: String, val text: String, val content: String)

    /**
     * Subtitle decoders are not in `decoder-list` (mpv lists only video and audio
     * decoders there), so text subtitle support is checked by decoding a file:
     * SubRip and WebVTT go through FFmpeg's decoders (mpv's lavc_conv), ASS
     * through libass. Every format is tried before failing.
     */
    @Test
    fun externalTextSubtitlesAreDecoded() {
        val cases = listOf(
            SubtitleCase(
                "srt", "subrip", "Hello SubRip",
                "1\n00:00:00,000 --> 00:00:10,000\nHello SubRip\n",
            ),
            SubtitleCase(
                "vtt", "webvtt", "Hello WebVTT",
                "WEBVTT\n\n00:00:00.000 --> 00:00:10.000\nHello WebVTT\n",
            ),
            SubtitleCase(
                "ass", "ass", "Hello ASS",
                """
                [Script Info]
                ScriptType: v4.00+
                PlayResX: 384
                PlayResY: 288

                [V4+ Styles]
                Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
                Style: Default,sans-serif,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1

                [Events]
                Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
                Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,Hello ASS
                """.trimIndent() + "\n",
            ),
        )

        val failures = mutableListOf<String>()
        // 3 s of audio under a subtitle shown from 0 to 10 s
        withWav("libmpv-test-subtitle-audio.wav", SAMPLE_RATE * 3) { wav ->
            for (case in cases)
                checkSubtitle(wav, case)?.let { failures += it }
        }
        if (failures.isNotEmpty())
            fail("${failures.size} of ${cases.size} subtitle formats failed:\n\n" + failures.joinToString("\n\n"))
    }

    /** Returns null on success, otherwise a description of what went wrong. */
    private fun checkSubtitle(wav: File, case: SubtitleCase): String? {
        val sub = File(TestMpv.cacheDir, "libmpv-test-subtitle.${case.extension}")
        try {
            sub.writeText(case.content)
            return TestMpv.withMpv("sub-files" to sub.absolutePath) { mpv ->
                val shownOrEnded = CountDownLatch(1)
                val shown = AtomicBoolean(false)
                val lastText = AtomicReference<String?>(null)
                val trackList = AtomicReference<String?>(null)
                val endFile = AtomicReference<String?>(null)

                mpv.addObserver(object : MPVLib.EventObserver {
                    override fun event(eventId: Int) {
                        // read here: the track list is gone once the file has ended
                        if (eventId == MPVLib.MpvEvent.MPV_EVENT_FILE_LOADED)
                            trackList.set(mpv.getPropertyString("track-list"))
                    }

                    override fun eventEndFile(reason: Int, error: Int) {
                        endFile.set("reason $reason, error $error")
                        shownOrEnded.countDown()
                    }

                    override fun eventProperty(property: String, value: String) {
                        if (property != "sub-text")
                            return
                        lastText.set(value)
                        if (value == case.text) {
                            shown.set(true)
                            shownOrEnded.countDown()
                        }
                    }
                })
                mpv.observeProperty("sub-text", MPVLib.MpvFormat.MPV_FORMAT_STRING)
                mpv.command(arrayOf("loadfile", wav.absolutePath))

                val signalled = TestMpv.await(shownOrEnded)
                val tracks = trackList.get()
                val subTrack = tracks?.let { json ->
                    val array = JSONArray(json)
                    (0 until array.length()).map { array.getJSONObject(it) }.firstOrNull { it.optString("type") == "sub" }
                }
                val problem = when {
                    !signalled -> "no '${case.text}' in sub-text and no end-file within ${TestMpv.TIMEOUT_SECONDS} s"
                    !shown.get() -> "file ended (${endFile.get()}) before sub-text showed '${case.text}'"
                    subTrack == null -> "no subtitle track"
                    subTrack.optString("codec") != case.codec -> "subtitle codec is not '${case.codec}'"
                    else -> null
                }
                problem?.let {
                    "${case.extension}: $it\n  last sub-text: ${describe(lastText.get())}\n  track-list at FILE_LOADED: $tracks"
                }
            }
        } finally {
            sub.delete()
        }
    }

    /**
     * A subtitle fixture from the test assets (`assets/subtitles`, whose README says how each
     * was made), with what mpv must report for it. [companions] are copied next to it.
     */
    private class SubtitleFixture(
        val file: String,
        val codec: String,
        val decoder: String,
        /** sub-end of the fixture's one subtitle, which starts at 0 s */
        val end: Double,
        /** sub-text, for a text format; null for a bitmap one */
        val text: String? = null,
        val companions: List<String> = emptyList(),
    )

    private fun copyAsset(name: String): File {
        val file = File(TestMpv.cacheDir, name)
        // the test APK's own assets, not the library's
        InstrumentationRegistry.getInstrumentation().context.assets.open("subtitles/$name").use { input ->
            file.outputStream().use { input.copyTo(it) }
        }
        return file
    }

    /**
     * Bitmap subtitles (PGS, DVD/VobSub) and MP4's text subtitles (`mov_text`): the formats
     * [externalTextSubtitlesAreDecoded] cannot write as text, from binary fixtures of a few
     * hundred bytes. Each is loaded as an external subtitle under a paused WAV.
     *
     * What this can honestly assert with `vo=null`. Nothing renders, so for the bitmap
     * formats there is no picture to look at and no `sub-text`. Asserted is:
     * - the track has the format's codec, and its `decoder` (which mpv sets only once it has
     *   found and opened a decoder for the selected track) is the build's FFmpeg decoder;
     * - the decoder produced the fixture's subtitle: `sub-start` and `sub-end`, which mpv
     *   takes from the subtitles the decoder returned (unavailable before), become
     *   available with the fixture's times. For PGS the end is the fixture's second display
     *   set (10 s); for VobSub it is the SPU's own stop-display delay, which only the decoder
     *   reads: the maximum, 65535 * 1024 / 90000 s, as FFmpeg's dvdsub encoder wrote it;
     * - and decoded it without an error in mpv's log (subtitle or FFmpeg code). This is what
     *   catches a broken bitmap: FFmpeg's PGS decoder still returns a subtitle, with its
     *   times, when the bitmap's data is damaged, and only logs "Insufficient RLE data" (a
     *   fixture damaged that way fails this test);
     * - for `mov_text`, a text format, `sub-text` shows the text, as for the other text
     *   formats.
     * Whether a bitmap would look right on screen is not checked.
     */
    @Test
    fun bitmapAndMp4TextSubtitlesAreDecoded() {
        val fixtures = listOf(
            SubtitleFixture("pgs.sup", codec = "hdmv_pgs_subtitle", decoder = "pgssub", end = 10.0),
            SubtitleFixture(
                "vobsub.idx", codec = "dvd_subtitle", decoder = "dvdsub", end = 65535 * 1024 / 90000.0,
                companions = listOf("vobsub.sub"),
            ),
            SubtitleFixture("mov_text.mp4", codec = "mov_text", decoder = "mov_text", end = 10.0, text = "Hello mov_text"),
        )
        val failures = mutableListOf<String>()
        withWav("libmpv-test-bitmap-subtitle-audio.wav", SAMPLE_RATE * 3) { wav ->
            for (fixture in fixtures) {
                val copies = (listOf(fixture.file) + fixture.companions).map(::copyAsset)
                try {
                    checkSubtitleFixture(wav, copies.first(), fixture)?.let { failures += it }
                } finally {
                    copies.forEach { it.delete() }
                }
            }
        }
        if (failures.isNotEmpty())
            fail("${failures.size} of ${fixtures.size} subtitle formats failed:\n\n" + failures.joinToString("\n\n"))
    }

    /** Returns null on success, otherwise a description of what went wrong. */
    private fun checkSubtitleFixture(wav: File, sub: File, fixture: SubtitleFixture): String? =
        TestMpv.withMpv("sub-files" to sub.absolutePath, "pause" to "yes") { mpv ->
            val errors = CopyOnWriteArrayList<String>()
            val logged = CopyOnWriteArrayList<String>()
            mpv.addLogObserver(object : MPVLib.LogObserver {
                override fun logMessage(prefix: String, level: Int, text: String) {
                    logged += text.trimEnd()
                    if (level <= MPVLib.MpvLogLevel.MPV_LOG_LEVEL_ERROR &&
                        (prefix.startsWith("sub") || prefix.startsWith("ffmpeg"))
                    ) errors += "[$prefix] ${text.trimEnd()}"
                }
            })
            val problems = mutableListOf<String>()
            var track: MpvNode? = null
            try {
                test {
                    load(mpv, wav.absolutePath)
                    track = mpv.getPropertyNode("track-list").list.orEmpty().firstOrNull { it["type"]?.string == "sub" }
                    // decoded while paused at 0 s, where the subtitle starts
                    eventually { runCatching { mpv.getPropertyNode("sub-end") }.getOrNull()?.double != null }
                }
            } catch (e: kotlinx.coroutines.TimeoutCancellationException) {
                problems += "sub-start/sub-end never became available: nothing was decoded"
            }
            // The log arrives on the event thread, after the decoder wrote it: reading `errors`
            // now could miss an error logged just before the subtitle became available. mpv
            // delivers its log in order, so once this marker has arrived, so has everything
            // logged before it.
            val barrier = "libmpv-test-log-barrier ${fixture.file}"
            mpv.command(arrayOf("print-text", barrier))
            test { eventually { barrier in logged } }
            val t = track
            when {
                t == null -> problems += "no subtitle track"
                t["selected"]?.boolean != true -> problems += "the subtitle track is not selected"
                t["codec"]?.string != fixture.codec -> problems += "codec is not '${fixture.codec}'"
                t["decoder"]?.string != fixture.decoder -> problems += "decoder is not '${fixture.decoder}'"
            }
            val start = mpv.getPropertyDouble("sub-start")
            val end = mpv.getPropertyDouble("sub-end")
            if (end != null && (start == null || Math.abs(start) > 0.01 || Math.abs(end - fixture.end) > 0.01))
                problems += "sub-start $start and sub-end $end, expected 0.0 and ${fixture.end}"
            if (fixture.text != null) {
                val text = mpv.getPropertyString("sub-text")
                if (text != fixture.text) problems += "sub-text is ${describe(text)}, expected '${fixture.text}'"
            }
            if (errors.isNotEmpty()) problems += "errors logged: $errors"
            if (problems.isEmpty()) null
            else "${fixture.file}: " + problems.joinToString("; ") + "\n  track: $t"
        }

    /** The string plus its code points, so an encoding mismatch is visible in CI output. */
    private fun describe(s: String?): String =
        if (s == null) "null"
        else "\"$s\" [" + s.codePoints().toArray().joinToString(" ") { "U+%04X".format(it) } + "]"
}
