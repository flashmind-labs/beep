package com.flashmind.neobeep

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.ByteArrayOutputStream
import java.util.concurrent.TimeUnit

class HttpApi(private val config: ConnectionSettings) : ServerApi {
    companion object {
        private val client = OkHttpClient.Builder()
            .connectTimeout(10, TimeUnit.SECONDS)
            .readTimeout(90, TimeUnit.SECONDS)
            .writeTimeout(30, TimeUnit.SECONDS)
            .build()
        private val JSON = "application/json".toMediaType()
    }

    override suspend fun request(method: String, path: String, body: String, onChunk: (ByteArray) -> Unit): Int =
        withContext(Dispatchers.IO) {
            val url = "http://${config.address.trim()}$path"
            val builder = Request.Builder().url(url)
            if (config.token.isNotBlank()) {
                builder.header("Authorization", "Bearer ${config.token.trim()}")
            }
            val requestBody = when {
                method == "GET" -> null
                body.isNotEmpty() -> body.toRequestBody(JSON)
                method in listOf("POST", "PUT", "PATCH") -> "".toRequestBody(JSON)
                else -> null
            }
            builder.method(method, requestBody)

            val call = client.newCall(builder.build())
            val response = call.execute()
            val status = response.code
            response.body?.byteStream()?.use { stream ->
                val buf = ByteArray(8192)
                while (true) {
                    val n = stream.read(buf)
                    if (n == -1) break
                    onChunk(buf.copyOf(n))
                }
            }
            status
        }

    override suspend fun json(method: String, path: String, body: String): String {
        val output = ByteArrayOutputStream()
        val status = request(method, path, body) { output.write(it) }
        checkStatus(status)
        return output.toString(Charsets.UTF_8.name())
    }

    override fun checkStatus(status: Int) = defaultCheckStatus(status)
}
