package dev.jdtech.mpv.r8test

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The library's consumer ProGuard rules under R8: this app is minified (see
 * build.gradle.kts) and [Probe] uses the library on every path where native code calls
 * into Kotlin by name. A class or member that the rules fail to keep is renamed or removed,
 * and the step that needs it fails (most of them at once, in MPVLib.create, which looks up
 * every callback).
 */
@RunWith(AndroidJUnit4::class)
class MinifiedLibraryTest {
    @Test
    fun theLibraryWorksMinified() {
        // R8 processed the library: a class of it that no rule keeps is not there by its name
        // (looked up from here, in the test APK, which R8 does not optimise: in the app, R8
        // would resolve a constant Class.forName to the renamed class)
        val unkept = "dev.jdtech.mpv.internal.NodeCodec"
        val found = runCatching { Class.forName(unkept, false, Probe::class.java.classLoader) }.isSuccess
        assertFalse("$unkept is in the app: the library was not minified, so this test proves nothing", found)
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        assertEquals(Probe.STEPS, Probe.run(context))
    }
}
