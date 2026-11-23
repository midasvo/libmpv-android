package dev.jdtech.mpv

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.lang.reflect.InvocationTargetException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MPVLibLifecycleTest {

    private fun createInitialized(): MPVLib {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val mpv = MPVLib.create(context) ?: throw AssertionError("MPVLib.create returned null")
        assertEquals(0, mpv.setOptionString("vo", "null"))
        assertEquals(0, mpv.setOptionString("ao", "null"))
        mpv.init()
        return mpv
    }

    private fun assertDestroyRefused(thrown: Throwable?) {
        assertTrue("destroy() in a callback should throw IllegalStateException, threw $thrown",
            thrown is IllegalStateException)
        assertTrue("unexpected message: ${thrown!!.message}",
            thrown.message!!.contains("another thread"))
    }

    @Test
    fun destroyInObserverCallbackThrowsAndInstanceStaysUsable() {
        val mpv = createInitialized()
        try {
            val attempted = AtomicBoolean(false)
            val thrown = AtomicReference<Throwable?>()
            val firstEvent = CountDownLatch(1)
            val secondEvent = CountDownLatch(1)

            mpv.addObserver(object : MPVLib.EventObserver {
                override fun eventProperty(property: String, value: Double) {
                    if (property != "volume")
                        return
                    if (attempted.compareAndSet(false, true)) {
                        try {
                            mpv.destroy()
                        } catch (t: Throwable) {
                            thrown.set(t)
                        }
                        firstEvent.countDown()
                    } else {
                        secondEvent.countDown()
                    }
                }
            })

            // mpv sends an initial change event when a property is observed
            mpv.observeProperty("volume", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
            assertTrue("no initial volume event received", firstEvent.await(5, TimeUnit.SECONDS))
            assertDestroyRefused(thrown.get())

            // the instance and its event thread still work
            assertNotNull(mpv.getPropertyString("mpv-version"))
            mpv.setPropertyDouble("volume", 50.0)
            assertTrue("event thread stopped after the refused destroy()",
                secondEvent.await(5, TimeUnit.SECONDS))
        } finally {
            mpv.destroy()
        }
        assertThrows(IllegalStateException::class.java) { mpv.getPropertyString("mpv-version") }
    }

    @Test
    fun destroyInLogCallbackThrows() {
        val mpv = createInitialized()
        try {
            val attempted = AtomicBoolean(false)
            val thrown = AtomicReference<Throwable?>()
            val called = CountDownLatch(1)

            mpv.addLogObserver(object : MPVLib.LogObserver {
                override fun logMessage(prefix: String, level: Int, text: String) {
                    if (!attempted.compareAndSet(false, true))
                        return
                    try {
                        mpv.destroy()
                    } catch (t: Throwable) {
                        thrown.set(t)
                    }
                    called.countDown()
                }
            })
            mpv.command(arrayOf("print-text", "libmpv lifecycle test"))

            assertTrue("no log message received", called.await(5, TimeUnit.SECONDS))
            assertDestroyRefused(thrown.get())
            assertNotNull(mpv.getPropertyString("mpv-version"))
        } finally {
            mpv.destroy()
        }
    }

    @Test
    fun callsAfterDestroyThrow() {
        val mpv = createInitialized()
        mpv.destroy()

        val calls = listOf<Pair<String, () -> Unit>>(
            "init" to { mpv.init() },
            "command" to { mpv.command(arrayOf("stop")) },
            "commandWithResult" to { mpv.commandWithResult(arrayOf("stop")) },
            "requestLogMessages" to { mpv.requestLogMessages("warn") },
            "setOptionString" to { mpv.setOptionString("vo", "null") },
            "getPropertyString" to { mpv.getPropertyString("mpv-version") },
            "getPropertyLong" to { mpv.getPropertyLong("volume") },
            "setPropertyDouble" to { mpv.setPropertyDouble("volume", 50.0) },
            "setPropertyString" to { mpv.setPropertyString("pause", "yes") },
            "observeProperty" to { mpv.observeProperty("volume", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE) },
            "detachSurface" to { mpv.detachSurface() },
        )
        for ((name, call) in calls) {
            try {
                call()
                throw AssertionError("$name after destroy() did not throw")
            } catch (e: IllegalStateException) {
                // expected
            }
        }
    }

    @Test
    fun secondDestroyIsNoOp() {
        val mpv = createInitialized()
        mpv.destroy()
        mpv.destroy()

        // also without init()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val uninitialized = MPVLib.create(context) ?: throw AssertionError("MPVLib.create returned null")
        uninitialized.destroy()
        uninitialized.destroy()
    }

    // Java callers can pass null where Kotlin declares String. The public methods
    // reject it before native code; call the native methods directly to check
    // that they throw instead of crashing.
    @Test
    fun nullStringInNativeCallThrowsNullPointerException() {
        val mpv = createInitialized()
        try {
            val ptrField = MPVLib::class.java.getDeclaredField("nativeInstance")
            ptrField.isAccessible = true
            val ptr = ptrField.getLong(mpv)

            fun callNative(name: String, vararg args: Any?) {
                val method = MPVLib::class.java.declaredMethods.single { it.name == name }
                method.isAccessible = true
                val e = assertThrows(InvocationTargetException::class.java) {
                    method.invoke(mpv, ptr, *args)
                }
                assertTrue("$name: expected NullPointerException, got ${e.cause}",
                    e.cause is NullPointerException)
            }

            callNative("nativeSetOptionString", "vo", null)
            callNative("nativeRequestLogMessages", null)
            callNative("nativeGetPropertyString", null)
            callNative("nativeSetPropertyDouble", null, 50.0)
            callNative("nativeSetPropertyString", "pause", null)
            callNative("nativeSetPropertyString", null, "yes")
            callNative("nativeObserveProperty", null, MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)

            assertNotNull(mpv.getPropertyString("mpv-version"))
        } finally {
            mpv.destroy()
        }
    }
}
