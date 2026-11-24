-keep class dev.jdtech.mpv.MPVLib { *; }
# Called from libplayer.so by name: the native methods of the node, async, flow and stream
# API, the stream adapters mpv's stream callbacks call, and the exception it throws.
-keep class dev.jdtech.mpv.internal.Native { *; }
-keep class dev.jdtech.mpv.internal.ProtocolAdapter { *; }
-keep class dev.jdtech.mpv.internal.StreamAdapter { *; }
-keep class dev.jdtech.mpv.MpvException { <init>(int, java.lang.String); }
