package dev.jdtech.mpv.internal

import dev.jdtech.mpv.MPVLib.MpvError

/** mpv's own text for an error code (`mpv_error_string` in mpv 0.42.0). */
internal fun describeMpvError(error: Int): String = when (error) {
    MpvError.MPV_ERROR_SUCCESS -> "success"
    MpvError.MPV_ERROR_EVENT_QUEUE_FULL -> "event queue full"
    MpvError.MPV_ERROR_NOMEM -> "memory allocation failed"
    MpvError.MPV_ERROR_UNINITIALIZED -> "core not initialized"
    MpvError.MPV_ERROR_INVALID_PARAMETER -> "invalid parameter"
    MpvError.MPV_ERROR_OPTION_NOT_FOUND -> "option not found"
    MpvError.MPV_ERROR_OPTION_FORMAT -> "unsupported format for accessing option"
    MpvError.MPV_ERROR_OPTION_ERROR -> "error setting option"
    MpvError.MPV_ERROR_PROPERTY_NOT_FOUND -> "property not found"
    MpvError.MPV_ERROR_PROPERTY_FORMAT -> "unsupported format for accessing property"
    MpvError.MPV_ERROR_PROPERTY_UNAVAILABLE -> "property unavailable"
    MpvError.MPV_ERROR_PROPERTY_ERROR -> "error accessing property"
    MpvError.MPV_ERROR_COMMAND -> "error running command"
    MpvError.MPV_ERROR_LOADING_FAILED -> "loading failed"
    MpvError.MPV_ERROR_AO_INIT_FAILED -> "audio output initialization failed"
    MpvError.MPV_ERROR_VO_INIT_FAILED -> "video output initialization failed"
    MpvError.MPV_ERROR_NOTHING_TO_PLAY -> "no audio or video data played"
    MpvError.MPV_ERROR_UNKNOWN_FORMAT -> "unrecognized file format"
    MpvError.MPV_ERROR_UNSUPPORTED -> "not supported"
    MpvError.MPV_ERROR_NOT_IMPLEMENTED -> "operation not implemented"
    MpvError.MPV_ERROR_GENERIC -> "something happened"
    else -> "unknown error"
}
