package com.dipecs.collector.actions

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AccessibleContentPrefetcherTest {
    @Test
    fun parseUrlTargetAcceptsHttpsUrl() {
        val target = AccessibleContentPrefetcher.PrefetchTarget.parse("url:https://example.test/feed.json")

        assertEquals("url", target.kind)
        assertEquals("https://example.test/feed.json", target.value)
        assertTrue(target.cacheFileName().endsWith(".json"))
    }

    @Test(expected = IllegalArgumentException::class)
    fun parseUrlTargetRejectsHttpUrl() {
        AccessibleContentPrefetcher.PrefetchTarget.parse("url:http://example.test/feed.json")
    }

    @Test(expected = IllegalArgumentException::class)
    fun parseUrlTargetRejectsLocalhost() {
        AccessibleContentPrefetcher.PrefetchTarget.parse("url:https://localhost/feed.json")
    }

    @Test(expected = IllegalArgumentException::class)
    fun parseUrlTargetRejectsLoopbackAddress() {
        AccessibleContentPrefetcher.PrefetchTarget.parse("url:https://127.0.0.1/feed.json")
    }

    @Test(expected = IllegalArgumentException::class)
    fun parseUrlTargetRejectsPrivateAddress() {
        AccessibleContentPrefetcher.PrefetchTarget.parse("url:https://192.168.1.10/feed.json")
    }

    @Test(expected = IllegalStateException::class)
    fun parseUrlTargetRejectsUnsupportedKind() {
        AccessibleContentPrefetcher.PrefetchTarget.parse("pkg:com.example.app")
    }

    @Test
    fun parseUriTargetAcceptsContentUri() {
        val target = AccessibleContentPrefetcher.PrefetchTarget.parse("uri:content://downloads/document/1")

        assertEquals("uri", target.kind)
        assertEquals("content://downloads/document/1", target.value)
    }

    @Test(expected = IllegalArgumentException::class)
    fun parseUriTargetRejectsNonContentUri() {
        AccessibleContentPrefetcher.PrefetchTarget.parse("uri:file:///tmp/demo.txt")
    }
}
