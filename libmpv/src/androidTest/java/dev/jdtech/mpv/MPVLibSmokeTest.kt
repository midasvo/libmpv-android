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
            assertTrue(mpv.command(arrayOf("loadfile")) < 0)
            assertEquals(0, mpv.command(arrayOf("stop")))
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
            assertTrue("no initial volume event received", firstEvent.await(5, TimeUnit.SECONDS))

            // change it only after the first event, so mpv cannot coalesce the two
            mpv.setPropertyDouble("volume", 50.0)
            assertTrue("no second volume event received", secondEvent.await(5, TimeUnit.SECONDS))

            assertEquals(1, selfRemovingCalls.get())
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

            assertTrue("observer after a throwing one got no event", latch.await(5, TimeUnit.SECONDS))
        } finally {
            mpv.destroy()
        }
    }
}
