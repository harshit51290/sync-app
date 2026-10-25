package dev.phonesync.app

import android.content.Context
import android.os.Build

/// Everything the app remembers between launches.
class Prefs(context: Context) {

    private val prefs = context.applicationContext
        .getSharedPreferences("phonesync", Context.MODE_PRIVATE)
    private val history = context.applicationContext
        .getSharedPreferences("phonesync_history", Context.MODE_PRIVATE)

    /// How many files have been marked since the last size check.
    private var marks = 0

    var baseUrl: String
        get() = prefs.getString(KEY_URL, "") ?: ""
        set(value) = prefs.edit().putString(KEY_URL, normalizeUrl(value)).apply()

    var token: String
        get() = prefs.getString(KEY_TOKEN, "") ?: ""
        set(value) = prefs.edit().putString(KEY_TOKEN, value.trim()).apply()

    var deviceName: String
        get() = prefs.getString(KEY_NAME, null) ?: (Build.MODEL ?: "Phone")
        set(value) = prefs.edit().putString(KEY_NAME, value.trim().ifEmpty { Build.MODEL }).apply()

    var autoSync: Boolean
        get() = prefs.getBoolean(KEY_AUTO, true)
        set(value) = prefs.edit().putBoolean(KEY_AUTO, value).apply()

    var wifiOnly: Boolean
        get() = prefs.getBoolean(KEY_WIFI, true)
        set(value) = prefs.edit().putBoolean(KEY_WIFI, value).apply()

    /// True until the user restricts sync to a subset of folders.
    var allFolders: Boolean
        get() = prefs.getBoolean(KEY_ALL_FOLDERS, true)
        set(value) = prefs.edit().putBoolean(KEY_ALL_FOLDERS, value).apply()

    var folders: Set<String>
        get() = prefs.getStringSet(KEY_FOLDERS, emptySet()) ?: emptySet()
        set(value) = prefs.edit().putStringSet(KEY_FOLDERS, value).apply()

    var lastSyncAt: Long
        get() = prefs.getLong(KEY_LAST_AT, 0)
        set(value) = prefs.edit().putLong(KEY_LAST_AT, value).apply()

    var lastSyncCount: Int
        get() = prefs.getInt(KEY_LAST_COUNT, 0)
        set(value) = prefs.edit().putInt(KEY_LAST_COUNT, value).apply()

    fun isPaired() = baseUrl.isNotBlank()

    fun isSynced(key: String) = history.getBoolean(key, false)

    /// The laptop hashes every file it receives, so a resend is harmless — the
    /// local list only exists to avoid pushing the same bytes twice. Counting
    /// in memory keeps this O(1): reading the whole map per file would crawl
    /// once a phone has thousands of photos.
    fun markSynced(key: String) {
        history.edit().putBoolean(key, true).apply()
        if (++marks < 500) return
        marks = 0
        if (history.all.size > 25_000) history.edit().clear().apply()
    }

    fun resetHistory() {
        marks = 0
        history.edit().clear().apply()
    }

    private fun normalizeUrl(raw: String): String {
        var value = raw.trim()
        if (value.isEmpty()) return ""
        if (!value.startsWith("http://") && !value.startsWith("https://")) {
            value = "http://$value"
        }
        // The laptop hands out links like http://ip:8792/?k=<token>; pasting the
        // whole link into the address box must still leave a usable base URL.
        return value.substringBefore('?').trimEnd('/')
    }

    private companion object {
        const val KEY_URL = "base_url"
        const val KEY_TOKEN = "token"
        const val KEY_NAME = "device_name"
        const val KEY_AUTO = "auto_sync"
        const val KEY_WIFI = "wifi_only"
        const val KEY_ALL_FOLDERS = "all_folders"
        const val KEY_FOLDERS = "folders"
        const val KEY_LAST_AT = "last_sync_at"
        const val KEY_LAST_COUNT = "last_sync_count"
    }
}
