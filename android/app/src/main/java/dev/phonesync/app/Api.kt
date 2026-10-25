package dev.phonesync.app

import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okio.BufferedSink
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.TimeUnit

class HttpException(val code: Int, message: String) : IOException(message)

/// Thin client for the laptop receiver. The protocol is deliberately small:
/// open a session, stream the file in chunks, then ask the laptop to finish.
class Api(baseUrl: String, private val token: String) {

    private val base = baseUrl.trim().trimEnd('/')
    private val client = OkHttpClient.Builder()
        .connectTimeout(8, TimeUnit.SECONDS)
        .readTimeout(90, TimeUnit.SECONDS)
        .writeTimeout(90, TimeUnit.SECONDS)
        .retryOnConnectionFailure(false)
        .build()

    private val jsonType = "application/json; charset=utf-8".toMediaType()
    private val binType = "application/octet-stream".toMediaType()

    fun info(): JSONObject = call(Request.Builder().url("$base/api/info").get())

    fun session(
        name: String,
        relPath: String,
        size: Long,
        mtimeMs: Long,
        device: String,
    ): JSONObject {
        val payload = JSONObject()
            .put("name", name)
            .put("rel_path", relPath)
            .put("size", size)
            .put("mtime_ms", mtimeMs)
            .put("device", device)
            .put("platform", "android")
        return call(
            authed("/api/session").post(payload.toString().toRequestBody(jsonType)),
        )
    }

    fun putChunk(sessionId: String, offset: Long, buffer: ByteArray, length: Int): Long {
        val body = object : RequestBody() {
            override fun contentType() = binType
            override fun contentLength() = length.toLong()
            override fun writeTo(sink: BufferedSink) {
                sink.write(buffer, 0, length)
            }
        }
        val json = call(
            authed("/api/session/$sessionId")
                .header("x-offset", offset.toString())
                .put(body),
        )
        return json.optLong("offset", offset + length)
    }

    fun complete(sessionId: String): JSONObject = call(
        authed("/api/session/$sessionId/complete")
            .post("{}".toRequestBody(jsonType)),
    )

    fun cancel(sessionId: String) {
        runCatching { call(authed("/api/session/$sessionId").delete()) }
    }

    private fun authed(path: String) = Request.Builder().url("$base$path").header("x-sync-key", token)

    private fun call(builder: Request.Builder): JSONObject {
        client.newCall(builder.build()).execute().use { response ->
            val text = response.body?.string().orEmpty()
            if (!response.isSuccessful) {
                val reason = runCatching { JSONObject(text).optString("error") }
                    .getOrNull()
                    .orEmpty()
                throw HttpException(
                    response.code,
                    reason.ifBlank { text.take(160).ifBlank { "HTTP ${response.code}" } },
                )
            }
            return if (text.isBlank()) JSONObject() else JSONObject(text)
        }
    }
}
