package dev.phonesync.app

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager

class App : Application() {

    override fun onCreate() {
        super.onCreate()
        prefs = Prefs(this)
        val nm = getSystemService(NotificationManager::class.java)
        val channel = NotificationChannel(
            CHANNEL_SYNC,
            getString(R.string.app_name),
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = getString(R.string.auto_sync_hint)
            setShowBadge(false)
        }
        nm.createNotificationChannel(channel)
        SyncScheduler.ensure(this)
    }

    companion object {
        const val CHANNEL_SYNC = "sync"
        lateinit var prefs: Prefs
            private set
    }
}
