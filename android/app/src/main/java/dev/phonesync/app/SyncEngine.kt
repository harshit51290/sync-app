package dev.phonesync.app

import android.content.Context
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.text.format.Formatter
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.InputStream
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/// One sync run: look at MediaStore, push whatever the laptop has not seen yet.
/// Used by both the foreground service and the background worker.
object SyncEngine {

    enum class Phase { IDLE, SCANNING, SYNCING, DONE, OFFLINE, PAUSED, UNPAIRED, NO_ACCESS, ERROR }

    data class State(
        val phase: Phase = Phase.IDLE,
        val title: String = "",
        val detail: String = "",
        val done: Int = 0,
        val total: Int = 0,
        val sent: Int = 0,
        val skipped: Int = 0,
        val failed: Int = 0,
        val bytesSent: Long = 0,
        val bytesTotal: Long = 0,
    ) {
        val running: Boolean get() = phase == Phase.SCANNING || phase == Phase.SYNCING
    }

    data class Summary(val sent: Int, val skipped: Int, val failed: Int, val aborted: Boolean)

    enum class LogKind { OK, SKIP, BAD }

    data class LogLine(val text: String, val meta: String, val kind: LogKind)

    val state = MutableStateFlow(State())
    val log = MutableStateFlow<List<LogLine>>(emptyList())

    @Volatile
    private var stopRequested = false

    private val gate = Mutex()

    fun cancel() {
        stopRequested = true
    }

    suspend fun sync(context: Context, reason: String): Summary = gate.withLock {
        try {
            runSync(context, reason)
        } finally {
            stopRequested = false
        }
    }

    private suspend fun runSync(context: Context, reason: String): Summary =
        withContext(Dispatchers.IO) {
            val app = context.applicationContext
            val prefs = App.prefs
            log.value = emptyList()

            if (!prefs.isPaired()) {
                state.value = State(
                    Phase.UNPAIRED,
                    context.getString(R.string.not_paired),
                    context.getString(R.string.not_paired_long),
                )
                return@withContext Summary(0, 0, 0, true)
            }
            if (!hasMediaAccess(context)) {
                state.value = State(
                    Phase.NO_ACCESS,
                    context.getString(R.string.permission_title),
                    context.getString(R.string.permission_body),
                )
                return@withContext Summary(0, 0, 0, true)
            }
            if (!online(context, prefs.wifiOnly)) {
                state.value = State(
                    Phase.OFFLINE,
                    context.getString(R.string.paused_offline),
                    context.getString(R.string.wifi_only_hint),
                )
                return@withContext Summary(0, 0, 0, true)
            }

            state.value = State(Phase.SCANNING, context.getString(R.string.scanning), "")
            val everything = MediaScanner.scan(context)
            val allowed = if (prefs.allFolders || prefs.folders.isEmpty()) {
                everything
            } else {
                val chosen = prefs.folders
                everything.filter { chosen.contains(it.folder) }
            }
            val pending = allowed.filter { !prefs.isSynced(it.key) }
            val skipped = allowed.size - pending.size

            if (pending.isEmpty()) {
                prefs.lastSyncAt = System.currentTimeMillis()
                prefs.lastSyncCount = 0
                state.value = State(
                    Phase.DONE,
                    context.getString(R.string.ready),
                    if (allowed.isEmpty()) {
                        context.getString(R.string.empty_folders)
                    } else {
                        context.getString(R.string.nothing_new)
                    },
                    skipped = skipped,
                )
                return@withContext Summary(0, skipped, 0, false)
            }

            val api = Api(prefs.baseUrl, prefs.token)
            val totalBytes = pending.sumOf { it.size }
            var sent = 0
            var failed = 0
            var bytes = 0L
            var aborted = false

            for ((index, item) in pending.withIndex()) {
                if (stopRequested) {
                    aborted = true
                    break
                }
                state.value = State(
                    Phase.SYNCING,
                    context.getString(R.string.uploading, item.name),
                    "${index + 1} / ${pending.size} · ${Formatter.formatShortFileSize(context, item.size)}",
                    done = index,
                    total = pending.size,
                    sent = sent,
                    skipped = skipped,
                    failed = failed,
                    bytesSent = bytes,
                    bytesTotal = totalBytes,
                )
                try {
                    upload(context, api, prefs, item)
                    prefs.markSynced(item.key)
                    sent++
                    bytes += item.size
                    addLog(item.name, Formatter.formatShortFileSize(context, item.size), LogKind.OK)
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (error: Exception) {
                    failed++
                    addLog(item.name, error.message ?: "failed", LogKind.BAD)
                    if (error is HttpException && error.code == 503) {
                        state.value = State(
                            Phase.PAUSED,
                            context.getString(R.string.paused_laptop),
                            context.getString(R.string.paused_laptop),
                            total = pending.size,
                            sent = sent,
                            skipped = skipped,
                            failed = failed,
                        )
                        aborted = true
                        break
                    }
                    if (error is HttpException && (error.code == 401 || error.code == 403)) {
                        state.value = State(
                            Phase.ERROR,
                            context.getString(R.string.sync_error, error.message ?: "401"),
                            context.getString(R.string.connection_bad),
                            total = pending.size,
                            sent = sent,
                            skipped = skipped,
                            failed = failed,
                        )
                        aborted = true
                        break
                    }
                }
            }

            prefs.lastSyncAt = System.currentTimeMillis()
            prefs.lastSyncCount = sent

            if (!aborted) {
                state.value = State(
                    Phase.DONE,
                    if (sent > 0) context.getString(R.string.sent_n, sent) else context.getString(R.string.ready),
                    if (failed > 0) context.getString(R.string.run_failed) + " $failed" else "",
                    done = pending.size,
                    total = pending.size,
                    sent = sent,
                    skipped = skipped,
                    failed = failed,
                    bytesSent = bytes,
                    bytesTotal = totalBytes,
                )
            }
            Summary(sent, skipped, failed, aborted)
        }

    /// Opens an upload session, streams the file in the chunk size the laptop
    /// asked for, then lets the laptop verify and place it.
    private fun upload(context: Context, api: Api, prefs: Prefs, item: MediaItem) {
        val rel = item.relPath + item.name
        val session = api.session(item.name, rel, item.size, item.mtimeMs, prefs.deviceName)
        val id = session.getString("id")
        var offset = session.optLong("offset", 0)
        val chunkSize = session.optInt("chunk_size", DEFAULT_CHUNK)
            .coerceIn(64 * 1024, 32 * 1024 * 1024)

        val input = context.contentResolver.openInputStream(item.uri)
            ?: throw IllegalStateException("cannot open ${item.name}")
        input.use { stream ->
            if (offset > 0) skipFully(stream, offset)
            val buffer = ByteArray(chunkSize)
            while (offset < item.size) {
                if (stopRequested) return
                val want = minOf(chunkSize.toLong(), item.size - offset).toInt()
                var read = 0
                while (read < want) {
                    val n = stream.read(buffer, read, want - read)
                    if (n < 0) break
                    read += n
                }
                if (read <= 0) break
                offset = api.putChunk(id, offset, buffer, read)
            }
        }
        if (offset < item.size) {
            throw IllegalStateException("file changed while uploading")
        }
        api.complete(id)
    }

    private fun skipFully(stream: InputStream, count: Long) {
        var left = count
        val scratch = ByteArray(64 * 1024)
        while (left > 0) {
            val n = stream.read(scratch, 0, minOf(scratch.size.toLong(), left).toInt())
            if (n < 0) return
            left -= n
        }
    }

    private fun addLog(text: String, meta: String, kind: LogKind) {
        val stamp = SimpleDateFormat("HH:mm", Locale.getDefault()).format(Date())
        val line = LogLine(text, "$meta · $stamp", kind)
        log.value = (listOf(line) + log.value).take(60)
    }

    private fun hasMediaAccess(context: Context): Boolean {
        val permissions = if (android.os.Build.VERSION.SDK_INT >= 33) {
            listOf(
                android.Manifest.permission.READ_MEDIA_IMAGES,
                android.Manifest.permission.READ_MEDIA_VIDEO,
            )
        } else {
            listOf(android.Manifest.permission.READ_EXTERNAL_STORAGE)
        }
        return permissions.any {
            context.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED
        }
    }

    private fun online(context: Context, wifiOnly: Boolean): Boolean {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val network = cm.activeNetwork ?: return false
        val caps = cm.getNetworkCapabilities(network) ?: return false
        if (!caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)) return false
        if (!wifiOnly) return true
        return caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
    }

    private const val DEFAULT_CHUNK = 8 * 1024 * 1024
}
