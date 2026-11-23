package dev.jdtech.mpv

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Regression test for what the build contains, read from mpv's own lists, so a
 * dependency bump that silently drops a codec, container or protocol fails CI.
 *
 * The expected names come from the sources of the pinned versions (mpv 0.42.0,
 * FFmpeg 9.1) and the configure flags in buildscripts/scripts/ffmpeg.sh and mpv.sh.
 * Each test collects everything that is missing and fails once, printing the
 * complete actual list, so one CI run is enough to correct a wrong name.
 *
 * Subtitle decoders are not here: mpv's `decoder-list` holds only video and audio
 * decoders. [MPVLibPlaybackTest.externalTextSubtitlesAreDecoded] covers text subtitles.
 */
@RunWith(AndroidJUnit4::class)
class MPVLibCapabilitiesTest {

    /** One `decoder-list` entry: mpv's codec name (FFmpeg's codec descriptor name) and the decoder. */
    private data class Decoder(val codec: String, val driver: String) {
        override fun toString() = "$codec/$driver"
    }

    /** `decoder-list` is node-typed, so getPropertyString returns it as JSON. */
    private fun decoders(mpv: MPVLib): List<Decoder> {
        val json = mpv.getPropertyString("decoder-list") ?: throw AssertionError("decoder-list unavailable")
        val array = JSONArray(json)
        return (0 until array.length()).map {
            val entry = array.getJSONObject(it)
            Decoder(entry.getString("codec"), entry.getString("driver"))
        }
    }

    private fun assertDecoders(expected: List<Decoder>): Unit = TestMpv.withMpv { mpv ->
        val actual = decoders(mpv)
        val missing = expected.filterNot { it in actual }
        if (missing.isNotEmpty())
            fail(
                "missing decoders (codec/driver): $missing\n" +
                    "actual decoder-list, ${actual.size} entries:\n" + actual.joinToString("\n")
            )
    }

    /**
     * Reads a string-list property. Through getPropertyString mpv joins it with ','
     * (not JSON), and some FFmpeg demuxer names contain commas themselves, such as
     * "matroska,webm", so entries are matched as runs of consecutive items.
     */
    private fun assertListContains(property: String, expected: List<String>): Unit = TestMpv.withMpv { mpv ->
        val joined = mpv.getPropertyString(property) ?: throw AssertionError("$property unavailable")
        val items = TestMpv.splitList(joined)
        val padded = ",$joined,"
        val missing = expected.filterNot { padded.contains(",$it,") }
        if (missing.isNotEmpty())
            fail(
                "missing from $property: $missing\n" +
                    "actual $property, ${items.size} items:\n" + items.joinToString("\n")
            )
    }

    @Test
    fun videoDecoders() = assertDecoders(
        listOf(
            Decoder("h264", "h264"),
            Decoder("hevc", "hevc"),
            // FFmpeg's own "av1" decoder needs a hwaccel, so AV1 software decoding is libdav1d
            Decoder("av1", "libdav1d"),
            Decoder("vp9", "vp9"),
            Decoder("vp8", "vp8"),
            Decoder("mpeg2video", "mpeg2video"),
            Decoder("mpeg4", "mpeg4"),
            Decoder("vc1", "vc1"),
        )
    )

    /** --enable-jni --enable-mediacodec; listed whether or not the device has the codec. */
    @Test
    fun mediaCodecDecoders() = assertDecoders(
        listOf(
            Decoder("h264", "h264_mediacodec"),
            Decoder("hevc", "hevc_mediacodec"),
            Decoder("av1", "av1_mediacodec"),
            Decoder("vp9", "vp9_mediacodec"),
            Decoder("vp8", "vp8_mediacodec"),
            Decoder("mpeg2video", "mpeg2_mediacodec"),
            Decoder("mpeg4", "mpeg4_mediacodec"),
        )
    )

    @Test
    fun audioDecoders() = assertDecoders(
        listOf(
            Decoder("aac", "aac"),
            Decoder("ac3", "ac3"),
            Decoder("eac3", "eac3"),
            Decoder("dts", "dca"),
            Decoder("truehd", "truehd"),
            Decoder("flac", "flac"),
            Decoder("opus", "opus"),
            Decoder("vorbis", "vorbis"),
            Decoder("mp3", "mp3float"),
            Decoder("pcm_s16le", "pcm_s16le"),
        )
    )

    /** libavformat demuxers; mpv also has native ones (Matroska among them) that are not listed here. */
    @Test
    fun lavfDemuxers() = assertListContains(
        "demuxer-lavf-list",
        listOf(
            "matroska,webm",
            "mov,mp4,m4a,3gp,3g2,mj2",
            "mpegts",
            "avi",
            "mpeg", // MPEG-PS (VOB)
            "hls",
            "dash", // needs libxml2
            "flac",
            "ogg",
            "mp3",
            "aac",
            "ac3",
            "eac3",
            "dts",
            "truehd",
            "wav",
            "srt",
            "webvtt",
            "ass",
            "sup", // PGS
            "vobsub",
        )
    )

    /**
     * mpv's protocol list: its own streams, libcurl's schemes and FFmpeg's protocols.
     * "http"/"https" may come from either libcurl or FFmpeg; "tls" only from FFmpeg
     * (mbedtls) and "ftps" only from libcurl (mbedtls), so TLS is checked in both.
     */
    @Test
    fun protocols() = assertListContains(
        "protocol-list",
        listOf(
            "file",
            "http",
            "https",
            "tls",
            "tcp",
            "crypto", // HLS AES-128
            "ftps",
        )
    )

    @Test
    fun versionProperties() = TestMpv.withMpv { mpv ->
        val problems = mutableListOf<String>()
        val values = mutableListOf<String>()
        for (property in listOf("mpv-version", "ffmpeg-version", "libplacebo-version")) {
            val value = mpv.getPropertyString(property)
            values += "$property = $value"
            if (value.isNullOrBlank())
                problems += "$property is empty or unavailable"
        }
        if (mpv.getPropertyString("mpv-version")?.startsWith("mpv ") != true)
            problems += "mpv-version does not start with \"mpv \""

        // ass_library_version(): an int64 such as 0x01705000 for 0.17.5
        val libass = mpv.getPropertyLong("libass-version")
        values += "libass-version = " + (libass?.let { "0x%08x".format(it) } ?: "null")
        if (libass == null || libass <= 0)
            problems += "libass-version is not a positive number"

        assertTrue(problems.joinToString("\n") + "\nvalues:\n" + values.joinToString("\n"), problems.isEmpty())
    }
}
