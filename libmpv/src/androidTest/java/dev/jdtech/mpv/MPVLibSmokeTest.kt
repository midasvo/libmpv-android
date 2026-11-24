package dev.jdtech.mpv

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MPVLibSmokeTest {

    private fun createInitialized(): MPVLib {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val mpv = MPVLib.create(context) ?: throw AssertionError("MPVLib.create returned null")
        assertEquals(0, mpv.setOptionString("vo", "null"))
        assertEquals(0, mpv.setOptionString("ao", "null"))
        mpv.init()
        return mpv
    }

    private open class NoOpObserver : MPVLib.EventObserver {
        override fun eventProperty(property: String) {}
        override fun eventProperty(property: String, value: Long) {}
        override fun eventProperty(property: String, value: Double) {}
        override fun eventProperty(property: String, value: Boolean) {}
        override fun eventProperty(property: String, value: String) {}
        override fun event(eventId: Int) {}
    }

    @Test
    fun createInitDestroy() {
        val mpv = createInitialized()
        try {
            val version = mpv.getPropertyString("mpv-version")
            assertNotNull(version)
            assertFalse(version!!.isEmpty())

            // loadfile without its required argument must fail
            assertTrue(mpv.commandWithResult(arrayOf("loadfile")) < 0)
            assertEquals(0, mpv.commandWithResult(arrayOf("stop")))
            mpv.command(arrayOf("stop")) // the Unit-returning form must keep working
        } finally {
            mpv.destroy()
        }
    }

    @Test
    fun observerMayRemoveItselfInCallback() {
        val mpv = createInitialized()
        try {
            val selfRemovingCalls = AtomicInteger(0)
            val volumeEvents = AtomicInteger(0)
            val firstEvent = CountDownLatch(1)
            val secondEvent = CountDownLatch(1)

            val selfRemoving = object : NoOpObserver() {
                override fun eventProperty(property: String, value: Double) {
                    if (property == "volume") {
                        selfRemovingCalls.incrementAndGet()
                        mpv.removeObserver(this)
                    }
                }
            }
            val counting = object : NoOpObserver() {
                override fun eventProperty(property: String, value: Double) {
                    if (property == "volume") {
                        when (volumeEvents.incrementAndGet()) {
                            1 -> firstEvent.countDown()
                            2 -> secondEvent.countDown()
                        }
                    }
                }
            }
            mpv.addObserver(selfRemoving)
            mpv.addObserver(counting)

            // mpv sends an initial change event when a property is observed
            mpv.observeProperty("volume", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
            assertTrue("no initial volume event received", firstEvent.await(TestMpv.TIMEOUT_SECONDS, TimeUnit.SECONDS))

            // change it only after the first event, so mpv cannot coalesce the two
            mpv.setPropertyDouble("volume", 50.0)
            assertTrue("no second volume event received", secondEvent.await(TestMpv.TIMEOUT_SECONDS, TimeUnit.SECONDS))

            assertEquals(1, selfRemovingCalls.get())
        } finally {
            mpv.destroy()
        }
    }

    @Test
    fun endFileReachesBothCallbacks() {
        val mpv = createInitialized()
        try {
            val endFile = CountDownLatch(1)
            val endFileEvent = CountDownLatch(1)
            val endReason = AtomicInteger(-1)
            val endError = AtomicInteger(1)

            // Overrides only what it needs: the other callbacks have default bodies.
            val observer = object : MPVLib.EventObserver {
                override fun eventEndFile(reason: Int, error: Int) {
                    endReason.set(reason)
                    endError.set(error)
                    endFile.countDown()
                }

                override fun event(eventId: Int) {
                    // event(END_FILE) must still arrive, and after eventEndFile
                    if (eventId == MPVLib.MpvEvent.MPV_EVENT_END_FILE && endFile.count == 0L)
                        endFileEvent.countDown()
                }
            }
            mpv.addObserver(observer)

            mpv.command(arrayOf("loadfile", "/nonexistent/libmpv-smoke-test.mkv"))

            assertTrue("no eventEndFile received", endFile.await(TestMpv.TIMEOUT_SECONDS, TimeUnit.SECONDS))
            assertTrue("no event(END_FILE) after eventEndFile", endFileEvent.await(TestMpv.TIMEOUT_SECONDS, TimeUnit.SECONDS))
            assertEquals(MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_ERROR, endReason.get())
            assertTrue("error code should be negative, was ${endError.get()}", endError.get() < 0)
        } finally {
            mpv.destroy()
        }
    }

    @Test
    fun throwingObserverDoesNotBlockOthers() {
        val mpv = createInitialized()
        try {
            val latch = CountDownLatch(1)

            val throwing = object : NoOpObserver() {
                override fun eventProperty(property: String, value: Double) {
                    throw RuntimeException("observer failure (expected by the test)")
                }
            }
            val counting = object : NoOpObserver() {
                override fun eventProperty(property: String, value: Double) {
                    if (property == "volume")
                        latch.countDown()
                }
            }
            mpv.addObserver(throwing)
            mpv.addObserver(counting)

            mpv.observeProperty("volume", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)

            assertTrue("observer after a throwing one got no event", latch.await(TestMpv.TIMEOUT_SECONDS, TimeUnit.SECONDS))
        } finally {
            mpv.destroy()
        }
    }
}
