// The examples in README.md and on the site, compiled against the real API so that they
// cannot drift from it. Nothing here runs: the unit test build only compiles it.
//
// Each example is the region between `// example:<name>` and `// end:<name>`, exactly the
// lines a reader sees (indented here by the function around them; strip that indentation).
// The imports it shows are the region `<name>-imports`, commented out (strip the "// "):
// Kotlin allows each class to be imported only once per file, so the real imports are the
// list after the regions, and every import in a region must be one of them. What an example
// uses without defining it (mpv, url, scope, ...) is a parameter of its function or a stub
// at the end of the file.
@file:Suppress("unused", "UNUSED_VARIABLE", "UNUSED_PARAMETER")

package dev.jdtech.mpv.docs

// example:structured-values-imports
// import dev.jdtech.mpv.MpvNode
// import dev.jdtech.mpv.commandNode
// import dev.jdtech.mpv.getPropertyNode
// import dev.jdtech.mpv.setPropertyNode
// end:structured-values-imports

// example:suspend-functions-imports
// import dev.jdtech.mpv.MpvNode
// import dev.jdtech.mpv.commandAsync
// import dev.jdtech.mpv.getPropertyAsync
// import dev.jdtech.mpv.setPropertyAsync
// import kotlinx.coroutines.launch
// end:suspend-functions-imports

// example:flows-imports
// import dev.jdtech.mpv.MPVLib
// import dev.jdtech.mpv.MpvPlayerEvent
// import dev.jdtech.mpv.doublePropertyFlow
// import dev.jdtech.mpv.eventFlow
// import kotlinx.coroutines.launch
// end:flows-imports

// example:custom-protocols-imports
// import android.net.Uri
// import dev.jdtech.mpv.MpvStream
// import dev.jdtech.mpv.registerStreamProtocol
// import java.io.FileNotFoundException
// end:custom-protocols-imports

// example:auto-close-imports
// import dev.jdtech.mpv.MPVLib
// end:auto-close-imports

// example:typed-async-imports
// import dev.jdtech.mpv.getBooleanPropertyAsync
// import dev.jdtech.mpv.getDoublePropertyAsync
// import dev.jdtech.mpv.getStringPropertyAsync
// import kotlinx.coroutines.launch
// end:typed-async-imports

// example:errors-imports
// import dev.jdtech.mpv.MpvException
// import dev.jdtech.mpv.MpvNode
// import dev.jdtech.mpv.setPropertyNode
// end:errors-imports

// example:logcat-level-imports
// import dev.jdtech.mpv.MPVLib
// import dev.jdtech.mpv.setLogcatLevel
// end:logcat-level-imports

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import dev.jdtech.mpv.MPVLib
import dev.jdtech.mpv.MpvException
import dev.jdtech.mpv.MpvNode
import dev.jdtech.mpv.MpvPlayerEvent
import dev.jdtech.mpv.MpvStream
import dev.jdtech.mpv.commandAsync
import dev.jdtech.mpv.commandNode
import dev.jdtech.mpv.doublePropertyFlow
import dev.jdtech.mpv.eventFlow
import dev.jdtech.mpv.getBooleanPropertyAsync
import dev.jdtech.mpv.getDoublePropertyAsync
import dev.jdtech.mpv.getPropertyAsync
import dev.jdtech.mpv.getPropertyNode
import dev.jdtech.mpv.getStringPropertyAsync
import dev.jdtech.mpv.registerStreamProtocol
import dev.jdtech.mpv.setLogcatLevel
import dev.jdtech.mpv.setPropertyAsync
import dev.jdtech.mpv.setPropertyNode
import java.io.FileNotFoundException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch


/** README, "More of mpv's API": structured values. */
fun structuredValues(mpv: MPVLib, url: String) {
    // example:structured-values
    val tracks = mpv.getPropertyNode("track-list").list.orEmpty()
    val audio = tracks.filter { it["type"]?.string == "audio" }
    audio.firstOrNull()?.get("id")?.let { mpv.setPropertyNode("aid", it) }

    val result = mpv.commandNode(MpvNode.command("loadfile", url, "append"))
    val entryId = result["playlist_entry_id"]?.long
    // end:structured-values
}

/** README, "More of mpv's API": suspend functions. */
fun suspendFunctions(mpv: MPVLib, url: String, scope: CoroutineScope) {
    // example:suspend-functions
    scope.launch {
        mpv.commandAsync(MpvNode.command("loadfile", url))
        mpv.setPropertyAsync("pause", MpvNode.of(false))
        val volume = mpv.getPropertyAsync("volume").double
    }
    // end:suspend-functions
}

/** README, "More of mpv's API": flows. */
fun flows(mpv: MPVLib, scope: CoroutineScope) {
    // example:flows
    scope.launch {
        mpv.doublePropertyFlow("time-pos").collect { position -> showPosition(position) }
    }
    scope.launch {
        mpv.eventFlow().collect { event ->
            when (event) {
                is MpvPlayerEvent.EndFile ->
                    if (event.reason == MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_ERROR) showError(event.error)
                is MpvPlayerEvent.Dropped -> reloadState()
                else -> {}
            }
        }
    }
    // end:flows
}

/** README, "More of mpv's API": custom protocols. */
fun customProtocols(mpv: MPVLib, contentResolver: ContentResolver, contentUri: Uri) {
    // example:custom-protocols
    // plays content:// URIs as "local://content://..." through the app's ContentResolver
    mpv.registerStreamProtocol("local") { uri ->
        val input = contentResolver.openInputStream(Uri.parse(uri.removePrefix("local://")))
            ?: throw FileNotFoundException(uri)
        object : MpvStream {
            override fun read(buffer: ByteArray, offset: Int, length: Int) = input.read(buffer, offset, length)
            override fun close() = input.close()
        }
    }
    mpv.command(arrayOf("loadfile", "local://$contentUri"))
    // end:custom-protocols
}

/** New: MPVLib is AutoCloseable, so `use` destroys it. */
fun autoClose(context: Context) {
    // example:auto-close
    val version = MPVLib.create(context)?.use { mpv ->
        mpv.setOptionString("vo", "null")
        mpv.init()
        mpv.getPropertyString("mpv-version")
    } // destroyed here, also if the block throws
    // end:auto-close
}

/** New: the typed variants of getPropertyAsync. */
fun typedAsync(mpv: MPVLib, scope: CoroutineScope) {
    // example:typed-async
    scope.launch {
        val paused: Boolean = mpv.getBooleanPropertyAsync("pause")
        val position: Double = mpv.getDoublePropertyAsync("time-pos")
        val title: String = mpv.getStringPropertyAsync("media-title")
    }
    // end:typed-async
}

/** New: the setters that report mpv's error (the classic setPropertyXxx only log it). */
fun errors(mpv: MPVLib) {
    // example:errors
    try {
        mpv.setPropertyNode("speed", MpvNode.of(1.5))
    } catch (e: MpvException) {
        showError(e.error) // an MPVLib.MpvError code, e.g. MPV_ERROR_PROPERTY_FORMAT
    }
    // end:errors
}

/** New: mpv's log for the app, but not in logcat. */
fun logcatLevel(mpv: MPVLib, appLog: MutableList<String>) {
    // example:logcat-level
    // keep only warnings and errors in logcat; the LogObservers still get every message
    mpv.setLogcatLevel(MPVLib.MpvLogLevel.MPV_LOG_LEVEL_WARN)
    mpv.addLogObserver(object : MPVLib.LogObserver {
        override fun logMessage(prefix: String, level: Int, text: String) {
            appLog += "[$prefix] $text"
        }
    })
    // end:logcat-level
}

// What the examples call without defining it.
private fun showPosition(position: Double?) {}
private fun showError(error: Int) {}
private fun reloadState() {}
