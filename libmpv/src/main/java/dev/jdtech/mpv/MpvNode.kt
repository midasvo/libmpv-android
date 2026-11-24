package dev.jdtech.mpv

/**
 * A structured mpv value: the Kotlin model of mpv's `mpv_node`. Properties such as
 * `track-list`, `chapter-list` or `metadata`, and the results of commands, are trees of
 * these instead of JSON text.
 *
 * Each subclass is one of mpv's node formats. The accessors ([string], [boolean], [long],
 * [double], [list], [map], [bytes], and the `[]` operators) return `null` when the node
 * is of another kind, so a lookup reads like a path:
 * ```
 * val tracks = mpv.getPropertyNode("track-list").list.orEmpty()
 * val audio = tracks.filter { it["type"]?.string == "audio" }
 * val selectedId = audio.firstOrNull { it["selected"]?.boolean == true }?.get("id")?.long
 * ```
 * Nodes are immutable values; equal nodes compare equal. Maps keep the order mpv sent the
 * entries in, which mpv documents as unspecified.
 */
sealed class MpvNode {
    /** `MPV_FORMAT_NONE`: no value (also what an unavailable observed property reads as). */
    data object None : MpvNode()

    /** `MPV_FORMAT_STRING`, holding [value]. */
    data class StringValue(val value: String) : MpvNode()

    /** `MPV_FORMAT_FLAG`, holding [value]. */
    data class FlagValue(val value: Boolean) : MpvNode()

    /** `MPV_FORMAT_INT64`, holding [value]. */
    data class Int64Value(val value: Long) : MpvNode()

    /** `MPV_FORMAT_DOUBLE`, holding [value]. */
    data class DoubleValue(val value: Double) : MpvNode()

    /** `MPV_FORMAT_NODE_ARRAY`, holding [values] in order. The list must not be modified after it is passed here. */
    data class ArrayValue(val values: List<MpvNode>) : MpvNode()

    /** `MPV_FORMAT_NODE_MAP`, holding [entries] by key. The map must not be modified after it is passed here. */
    data class MapValue(val entries: Map<String, MpvNode>) : MpvNode()

    /**
     * `MPV_FORMAT_BYTE_ARRAY` (used by a few commands, such as `screenshot-raw`). Holds a
     * copy of the bytes; equality compares the contents.
     */
    class ByteArrayValue(bytes: ByteArray) : MpvNode() {
        private val data = bytes.copyOf()

        /** The number of bytes. */
        val size: Int get() = data.size

        /** A copy of the bytes. */
        fun toByteArray(): ByteArray = data.copyOf()

        override fun equals(other: Any?): Boolean =
            other is ByteArrayValue && data.contentEquals(other.data)

        override fun hashCode(): Int = data.contentHashCode()

        override fun toString(): String = "ByteArrayValue(size=${data.size})"
    }

    /** The string of a [StringValue], else null. */
    val string: String? get() = (this as? StringValue)?.value

    /** The value of a [FlagValue], else null. */
    val boolean: Boolean? get() = (this as? FlagValue)?.value

    /** The value of an [Int64Value], else null (a [DoubleValue] is not truncated to a Long). */
    val long: Long? get() = (this as? Int64Value)?.value

    /** The value of a [DoubleValue], or of an [Int64Value] converted as mpv converts it, else null. */
    val double: Double?
        get() = when (this) {
            is DoubleValue -> value
            is Int64Value -> value.toDouble()
            else -> null
        }

    /** The entries of an [ArrayValue], else null. */
    val list: List<MpvNode>? get() = (this as? ArrayValue)?.values

    /** The entries of a [MapValue], else null. */
    val map: Map<String, MpvNode>? get() = (this as? MapValue)?.entries

    /** A copy of the bytes of a [ByteArrayValue], else null. */
    val bytes: ByteArray? get() = (this as? ByteArrayValue)?.toByteArray()

    /** Whether this is [None]. */
    val isNone: Boolean get() = this === None

    /** The entry [key] of a [MapValue]; null if there is none or this is not a map. */
    operator fun get(key: String): MpvNode? = map?.get(key)

    /** Entry [index] of an [ArrayValue]; null if out of range or this is not an array. */
    operator fun get(index: Int): MpvNode? = list?.getOrNull(index)

    companion object {
        /**
         * The node for a plain Kotlin value: null is [None]; a [MpvNode] is itself; [String],
         * [Boolean], [Long]/[Int]/[Short]/[Byte], [Double]/[Float] and [ByteArray] map to the
         * matching node; a [List] (or other [Iterable], or an [Array]) becomes an
         * [ArrayValue] and a [Map] with String keys a [MapValue], converted recursively.
         *
         * @throws IllegalArgumentException for anything else
         */
        @JvmStatic
        fun of(value: Any?): MpvNode = when (value) {
            null -> None
            is MpvNode -> value
            is String -> StringValue(value)
            is Boolean -> FlagValue(value)
            is Long -> Int64Value(value)
            is Int -> Int64Value(value.toLong())
            is Short -> Int64Value(value.toLong())
            is Byte -> Int64Value(value.toLong())
            is Double -> DoubleValue(value)
            is Float -> DoubleValue(value.toDouble())
            is ByteArray -> ByteArrayValue(value)
            is Map<*, *> -> MapValue(
                value.entries.associateTo(LinkedHashMap()) { (k, v) ->
                    require(k is String) { "map keys must be strings, not $k" }
                    k to of(v)
                },
            )
            is Iterable<*> -> ArrayValue(value.map(::of))
            is Array<*> -> ArrayValue(value.map(::of))
            else -> throw IllegalArgumentException("no mpv node for ${value.javaClass.name}")
        }

        /**
         * A command with positional arguments, as `mpv_command_node` takes it: an array of
         * [name] followed by [args] (each converted with [of]). For example
         * `MpvNode.command("loadfile", path, "append")`.
         */
        @JvmStatic
        fun command(name: String, vararg args: Any?): MpvNode =
            ArrayValue(listOf(StringValue(name)) + args.map(::of))

        /**
         * A command with named arguments: a map with `_name` set to [name] and the [args]
         * (each converted with [of]) under mpv's argument names. For example
         * `MpvNode.namedCommand("loadfile", mapOf("url" to path, "flags" to "append"))`.
         * Not every command supports named arguments (mpv's input.rst says which do).
         */
        @JvmStatic
        fun namedCommand(name: String, args: Map<String, Any?>): MpvNode =
            MapValue(LinkedHashMap<String, MpvNode>().apply {
                put("_name", StringValue(name))
                for ((k, v) in args) put(k, of(v))
            })
    }
}
