package com.dipecs.collector.net

import org.junit.Assert.assertEquals
import org.junit.Test

class CloudUploaderTest {
    @Test(expected = IllegalArgumentException::class)
    fun validateUploadEndpointRejectsHttp() {
        CloudUploader.validateUploadEndpoint("http://example.test/collector")
    }

    @Test(expected = IllegalArgumentException::class)
    fun validateUploadEndpointRejectsLocalhost() {
        CloudUploader.validateUploadEndpoint("https://localhost/collector")
    }

    @Test(expected = IllegalArgumentException::class)
    fun validateUploadEndpointRejectsLoopbackIp() {
        CloudUploader.validateUploadEndpoint("https://127.0.0.1/collector")
    }

    @Test
    fun validateUploadEndpointAcceptsPublicHttpsHost() {
        val url = CloudUploader.validateUploadEndpoint("https://example.com/collector")

        assertEquals("https", url.protocol)
        assertEquals("example.com", url.host)
    }
}
