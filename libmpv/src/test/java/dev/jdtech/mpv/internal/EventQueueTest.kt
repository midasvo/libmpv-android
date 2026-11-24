package dev.jdtech.mpv.internal

import dev.jdtech.mpv.MpvPlayerEvent
import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.yield
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class EventQueueTest {
    private fun message(i: Int) = MpvPlayerEvent.ClientMessage(listOf("m$i"))

    private fun EventQueue.drain(): List<MpvPlayerEvent> = runBlocking {
        close()
        buildList { while (true) add(take() ?: break) }
    }

    @Test
    fun eventsComeOutInOrder() {
        val q = EventQueue(4)
        (1..4).forEach { q.offer(message(it)) }
        assertEquals((1..4).map(::message), q.drain())
    }

    @Test
    fun overflowIsCountedWhereItHappened() {
        val q = EventQueue(3)
        (1..10).forEach { q.offer(message(it)) }
        // the first three, then one marker for the seven that did not fit
        assertEquals(listOf(message(1), message(2), message(3), MpvPlayerEvent.Dropped(7)), q.drain())
    }

    @Test
    fun afterTheCollectorCatchesUpEventsAreKeptAgain() = runBlocking {
        val q = EventQueue(2)
        (1..5).forEach { q.offer(message(it)) } // 1, 2, Dropped(3)
        assertEquals(message(1), q.take())
        assertEquals(message(2), q.take())
        assertEquals(MpvPlayerEvent.Dropped(3), q.take())
        q.offer(message(6))
        assertEquals(listOf(message(6)), q.drain())
    }

    @Test
    fun takeWaitsForAnOfferAndCloseEndsIt() = runBlocking {
        val q = EventQueue(2)
        val next = async { q.take() }
        yield()
        assertFalse(next.isCompleted)
        q.offer(MpvPlayerEvent.Seek)
        assertEquals(MpvPlayerEvent.Seek, withTimeout(5_000) { next.await() })
        val last = async { q.take() }
        yield()
        q.close()
        assertNull(withTimeout(5_000) { last.await() })
        q.offer(MpvPlayerEvent.Seek) // after close: ignored
        assertNull(q.take())
    }
}
