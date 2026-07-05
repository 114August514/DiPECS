package com.dipecs.collector.debug

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class DebugMemoryPressureAllocatorTest {
    @Test
    fun allocateBoundedChunksTouchesRequestedBytes() {
        val chunks = DebugMemoryPressureAllocator.allocate(holdMb = 3, chunkMb = 2)

        assertEquals(2, chunks.size)
        assertEquals(2 * 1024 * 1024, chunks[0].size)
        assertEquals(1 * 1024 * 1024, chunks[1].size)
        assertTrue(chunks.all { it[0].toInt() == 1 })
    }

    @Test
    fun allocateRejectsNonPositiveInputs() {
        val chunks = DebugMemoryPressureAllocator.allocate(holdMb = 0, chunkMb = 1)

        assertTrue(chunks.isEmpty())
    }

    @Test
    fun allocateBestEffortReportsCompletedAllocation() {
        val result = DebugMemoryPressureAllocator.allocateBestEffort(holdMb = 3, chunkMb = 2)

        assertEquals(3L * 1024L * 1024L, result.requestedBytes)
        assertEquals(3L * 1024L * 1024L, result.heldBytes)
        assertTrue(result.complete)
        assertEquals(2, result.chunks.size)
    }
}
