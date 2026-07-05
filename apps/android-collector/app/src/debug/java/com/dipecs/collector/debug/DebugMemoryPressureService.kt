package com.dipecs.collector.debug

import android.app.Service
import android.content.Intent
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

object DebugMemoryPressureAllocator {
    private const val BYTES_PER_MB = 1024L * 1024L
    private const val PAGE_SIZE = 4096

    fun allocate(holdMb: Int, chunkMb: Int): MutableList<ByteArray> {
        return allocateBestEffort(holdMb, chunkMb).chunks
    }

    fun allocateBestEffort(holdMb: Int, chunkMb: Int): AllocationResult {
        val requestedBytes = if (holdMb > 0) holdMb.toLong() * BYTES_PER_MB else 0L
        if (holdMb <= 0 || chunkMb <= 0) {
            return AllocationResult(mutableListOf(), requestedBytes, complete = true, errorClass = null)
        }
        var remainingBytes = requestedBytes
        val maxChunkBytes = chunkMb.toLong() * BYTES_PER_MB
        val chunks = mutableListOf<ByteArray>()
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
            var offset = 0
            while (offset < chunk.size) {
                chunk[offset] = 1
                offset += PAGE_SIZE
            }
            chunks += chunk
            remainingBytes -= bytes
        }
        return AllocationResult(chunks, requestedBytes, complete, errorClass)
    }

    data class AllocationResult(
        val chunks: MutableList<ByteArray>,
        val requestedBytes: Long,
        val complete: Boolean,
        val errorClass: String?,
    ) {
        val heldBytes: Long = chunks.sumOf { it.size.toLong() }
    }
}

/**
 * Debug-only bounded RAM pressure source used by real-device evaluation scripts.
 * It lives in the `:pressure` process and is absent from release source sets.
 */
open class DebugMemoryPressureService : Service() {
    private val handler = Handler(Looper.getMainLooper())
    private val lock = Any()
    private var chunks: MutableList<ByteArray> = mutableListOf()
    private var worker: Thread? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action ?: ACTION_START) {
            ACTION_STOP -> stopPressure()
            else -> startPressure(intent ?: Intent())
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        stopPressure()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startPressure(intent: Intent) {
        stopPressure()
        val holdMb = intent.getIntExtra(EXTRA_HOLD_MB, DEFAULT_HOLD_MB)
            .coerceIn(1, MAX_HOLD_MB)
        val chunkMb = intent.getIntExtra(EXTRA_CHUNK_MB, DEFAULT_CHUNK_MB)
            .coerceIn(1, MAX_CHUNK_MB)
        val windowSecs = intent.getIntExtra(EXTRA_WINDOW_SECS, DEFAULT_WINDOW_SECS)
            .coerceIn(1, MAX_WINDOW_SECS)

        EventRepository.recordInternal(
            this,
            "debug_memory_pressure_starting",
            "Debug memory pressure starting",
            JSONObject()
                .put("holdMb", holdMb)
                .put("chunkMb", chunkMb)
                .put("windowSecs", windowSecs),
        )

        worker = Thread({
            runCatching {
                val result = DebugMemoryPressureAllocator.allocateBestEffort(holdMb, chunkMb)
                synchronized(lock) {
                    chunks = result.chunks
                }
                EventRepository.recordInternal(
                    this,
                    "debug_memory_pressure_started",
                    "Debug memory pressure started",
                    JSONObject()
                        .put("holdMb", holdMb)
                        .put("chunks", result.chunks.size)
                        .put("requestedBytes", result.requestedBytes)
                        .put("heldBytes", result.heldBytes)
                        .put("complete", result.complete)
                        .put("errorClass", result.errorClass ?: JSONObject.NULL),
                )
                handler.postDelayed({ stopSelf() }, windowSecs * 1000L)
            }.onFailure { error ->
                EventRepository.recordInternal(
                    this,
                    "debug_memory_pressure_failed",
                    error.message ?: "Debug memory pressure failed",
                    JSONObject().put("errorClass", error.javaClass.simpleName),
                )
                stopSelf()
            }
        }, "dipecs-debug-memory-pressure")
        worker?.start()
    }

    private fun stopPressure() {
        handler.removeCallbacksAndMessages(null)
        synchronized(lock) {
            chunks.clear()
        }
        worker = null
        EventRepository.recordInternal(
            this,
            "debug_memory_pressure_stopped",
            "Debug memory pressure stopped",
        )
    }

    companion object {
        const val ACTION_START = "com.dipecs.collector.debug.MEMORY_PRESSURE_START"
        const val ACTION_STOP = "com.dipecs.collector.debug.MEMORY_PRESSURE_STOP"
        const val EXTRA_HOLD_MB = "hold_mb"
        const val EXTRA_CHUNK_MB = "chunk_mb"
        const val EXTRA_WINDOW_SECS = "window_secs"

        const val DEFAULT_HOLD_MB = 64
        const val DEFAULT_CHUNK_MB = 8
        const val DEFAULT_WINDOW_SECS = 15
        const val MAX_HOLD_MB = 2048
        const val MAX_CHUNK_MB = 64
        const val MAX_WINDOW_SECS = 120
    }
}

class DebugMemoryPressureService1 : DebugMemoryPressureService()
class DebugMemoryPressureService2 : DebugMemoryPressureService()
class DebugMemoryPressureService3 : DebugMemoryPressureService()
class DebugMemoryPressureService4 : DebugMemoryPressureService()
class DebugMemoryPressureService5 : DebugMemoryPressureService()
class DebugMemoryPressureService6 : DebugMemoryPressureService()
class DebugMemoryPressureService7 : DebugMemoryPressureService()
