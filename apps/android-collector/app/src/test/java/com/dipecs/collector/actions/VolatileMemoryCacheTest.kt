package com.dipecs.collector.actions

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VolatileMemoryCacheTest {
    @After
    fun cleanup() {
        VolatileMemoryCache.clear()
    }

    @Test
    fun seedAllocatesTouchedChunksAndClearReleasesThem() {
        val seeded = VolatileMemoryCache.seed(requestedMb = 3, chunkMb = 2)

        assertEquals(3L * 1024L * 1024L, seeded.requestedBytes)
        assertEquals(3L * 1024L * 1024L, seeded.heldBytes)
        assertEquals(2, seeded.chunks)
        assertTrue(seeded.complete)
        assertEquals(3L * 1024L * 1024L, VolatileMemoryCache.heldBytes())

        val released = VolatileMemoryCache.clear()

        assertEquals(3L * 1024L * 1024L, released.releasedBytes)
        assertEquals(2, released.releasedChunks)
        assertEquals(0L, VolatileMemoryCache.heldBytes())
    }

    @Test
    fun seedIsBoundedToMaxHoldMb() {
        val seeded = VolatileMemoryCache.seed(
            requestedMb = VolatileMemoryCache.MAX_HOLD_MB + 128,
            chunkMb = VolatileMemoryCache.DEFAULT_CHUNK_MB,
        )

        assertEquals(
            VolatileMemoryCache.MAX_HOLD_MB.toLong() * 1024L * 1024L,
            seeded.requestedBytes,
        )
        assertEquals(seeded.requestedBytes, seeded.heldBytes)
        assertEquals(seeded.heldBytes, VolatileMemoryCache.heldBytes())
    }

    @Test
    fun seedRejectsNonPositiveInputWithoutChangingExistingCache() {
        VolatileMemoryCache.seed(requestedMb = 2, chunkMb = 1)

        val result = VolatileMemoryCache.seed(requestedMb = 0, chunkMb = 1)

        assertFalse(result.complete)
        assertEquals("invalid_size", result.errorClass)
        assertEquals(2L * 1024L * 1024L, VolatileMemoryCache.heldBytes())
    }

    @Test
    fun parseTargetMbAcceptsOptionalSuffix() {
        assertEquals(64, VolatileMemoryCache.parseTargetMb("cache:volatile"))
        assertEquals(32, VolatileMemoryCache.parseTargetMb("cache:volatile:32"))
        assertEquals(64, VolatileMemoryCache.parseTargetMb("own:volatile-cache"))
        assertEquals(48, VolatileMemoryCache.parseTargetMb("own:volatile-cache:48"))
    }

    @Test
    fun seedReplacesExistingVolatileCache() {
        VolatileMemoryCache.seed(requestedMb = 2, chunkMb = 1)

        val seeded = VolatileMemoryCache.seed(requestedMb = 1, chunkMb = 1)

        assertEquals(1L * 1024L * 1024L, seeded.heldBytes)
        assertEquals(1L * 1024L * 1024L, VolatileMemoryCache.heldBytes())
        assertEquals(1, VolatileMemoryCache.chunkCount())
    }
}
