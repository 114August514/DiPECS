package com.dipecs.collector.actions

object VolatileMemoryCache {
    private const val BYTES_PER_MB = 1024L * 1024L
    private const val PAGE_SIZE = 4096

    const val DEFAULT_HOLD_MB = 64
    const val DEFAULT_CHUNK_MB = 8
    const val MAX_HOLD_MB = 128
    const val MAX_CHUNK_MB = 16

    private val lock = Any()
    private var chunks: MutableList<ByteArray> = mutableListOf()

    fun seed(
        requestedMb: Int = DEFAULT_HOLD_MB,
        chunkMb: Int = DEFAULT_CHUNK_MB,
    ): SeedResult {
        if (requestedMb <= 0 || chunkMb <= 0) {
            return SeedResult(
                requestedBytes = 0,
                heldBytes = heldBytes(),
                chunks = chunkCount(),
                complete = false,
                errorClass = "invalid_size",
            )
        }

        val boundedMb = requestedMb.coerceAtMost(MAX_HOLD_MB)
        val boundedChunkMb = chunkMb.coerceIn(1, MAX_CHUNK_MB)
        val requestedBytes = boundedMb.toLong() * BYTES_PER_MB
        val maxChunkBytes = boundedChunkMb.toLong() * BYTES_PER_MB
        val allocated = mutableListOf<ByteArray>()
        var remainingBytes = requestedBytes
        var complete = true
        var errorClass: String? = null

        while (remainingBytes > 0) {
            val bytes = minOf(remainingBytes, maxChunkBytes)
            val chunk = try {
                ByteArray(bytes.toInt())
            } catch (error: OutOfMemoryError) {
                complete = false
                errorClass = error.javaClass.simpleName
                break
            }
            touchEveryPage(chunk)
            allocated += chunk
            remainingBytes -= bytes
        }

        synchronized(lock) {
            chunks.clear()
            chunks = allocated
        }

        return SeedResult(
            requestedBytes = requestedBytes,
            heldBytes = allocated.sumOf { it.size.toLong() },
            chunks = allocated.size,
            complete = complete,
            errorClass = errorClass,
        )
    }

    fun clear(): ClearResult {
        return synchronized(lock) {
            val releasedBytes = chunks.sumOf { it.size.toLong() }
            val releasedChunks = chunks.size
            chunks.clear()
            ClearResult(releasedBytes, releasedChunks)
        }
    }

    fun heldBytes(): Long = synchronized(lock) {
        chunks.sumOf { it.size.toLong() }
    }

    fun chunkCount(): Int = synchronized(lock) {
        chunks.size
    }

    fun parseTargetMb(target: String?): Int {
        val normalized = target?.trim().orEmpty()
        return normalized
            .substringAfterLast(':', missingDelimiterValue = "")
            .toIntOrNull()
            ?.coerceIn(1, MAX_HOLD_MB)
            ?: DEFAULT_HOLD_MB
    }

    private fun touchEveryPage(chunk: ByteArray) {
        var offset = 0
        while (offset < chunk.size) {
            chunk[offset] = 1
            offset += PAGE_SIZE
        }
        if (chunk.isNotEmpty()) {
            chunk[chunk.lastIndex] = 1
        }
    }

    data class SeedResult(
        val requestedBytes: Long,
        val heldBytes: Long,
        val chunks: Int,
        val complete: Boolean,
        val errorClass: String?,
    )

    data class ClearResult(
        val releasedBytes: Long,
        val releasedChunks: Int,
    )
}
