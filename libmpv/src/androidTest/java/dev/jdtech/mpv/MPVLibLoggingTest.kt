package dev.jdtech.mpv

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.jdtech.mpv.ApiTestSupport.eventually
import dev.jdtech.mpv.ApiTestSupport.test
import dev.jdtech.mpv.MPVLib.MpvLogLevel
import java.util.concurrent.CopyOnWriteArrayList
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test
import org.junit.runner.RunWith

/** mpv's log: the LogObservers, and the echo to logcat that setLogcatLevel controls. */
@RunWith(AndroidJUnit4::class)
class MPVLibLoggingTest {

    /** This process's lines under the tag `mpv` (an app may read its own log). */
    private fun logcat(): String {
        val process = ProcessBuilder("logcat", "-d", "-v", "raw", "-s", "mpv").redirectErrorStream(true).start()
        return process.inputStream.bufferedReader().use { it.readText() }.also { process.waitFor() }
    }

    @Test
    fun theLogcatEchoCanBeStoppedWithoutStoppingTheObservers() = TestMpv.withMpv { mpv ->
        val observed = CopyOnWriteArrayList<String>()
        mpv.addLogObserver(object : MPVLib.LogObserver {
            override fun logMessage(prefix: String, level: Int, text: String) {
                observed += text.trimEnd()
            }
        })
        val marker = "libmpv-test-log-${System.nanoTime()}"
        test {
            // print-text logs at level info, under the default "v" request
            suspend fun print(text: String) {
                mpv.command(arrayOf("print-text", text))
                eventually { text in observed }
            }

            // the default: echoed, as before
            print("$marker-default")
            eventually { "$marker-default" in logcat() }

            // warn: the observers still get an info message, logcat does not
            mpv.setLogcatLevel(MpvLogLevel.MPV_LOG_LEVEL_WARN)
            print("$marker-warn")
            mpv.setLogcatLevel(MpvLogLevel.MPV_LOG_LEVEL_NONE)
            print("$marker-none")

            // back to everything; once this one is in logcat, the two before it would be too
            // (one thread writes them in order), so their absence is not a matter of timing
            mpv.setLogcatLevel(MpvLogLevel.MPV_LOG_LEVEL_TRACE)
            print("$marker-trace")
            eventually { "$marker-trace" in logcat() }
            val log = logcat()
            assertFalse("echoed at warn", "$marker-warn" in log)
            assertFalse("echoed at none", "$marker-none" in log)
        }
        assertEquals(
            listOf("default", "warn", "none", "trace").map { "$marker-$it" },
            observed.filter { it.startsWith(marker) },
        )
    }

    @Test
    fun setLogcatLevelAfterDestroyThrows() {
        val mpv = TestMpv.create()
        mpv.setLogcatLevel(MpvLogLevel.MPV_LOG_LEVEL_WARN)
        mpv.destroy()
        assertThrows(IllegalStateException::class.java) { mpv.setLogcatLevel(MpvLogLevel.MPV_LOG_LEVEL_WARN) }
    }
}
