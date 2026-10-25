package dev.phonesync.app

import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch

/// Keeps a run alive while the user is watching, with a progress notification.
class SyncService : Service() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var job: Job? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            SyncEngine.cancel()
            return START_NOT_STICKY
        }

        startForeground(NOTIFICATION_ID, build(SyncEngine.state.value))
        if (job?.isActive != true) {
            job = scope.launch {
                val watcher = launch {
                    SyncEngine.state.collectLatest { state ->
                        notificationManager.notify(NOTIFICATION_ID, build(state))
                    }
                }
                try {
                    SyncEngine.sync(this@SyncService, "manual")
                } finally {
                    watcher.cancel()
                    stopForeground(STOP_FOREGROUND_REMOVE)
                    stopSelf()
                }
            }
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private fun build(state: SyncEngine.State): android.app.Notification {
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        val builder = NotificationCompat.Builder(this, App.CHANNEL_SYNC)
            .setSmallIcon(R.drawable.ic_bolt)
            .setContentTitle(state.title.ifBlank { getString(R.string.app_name) })
            .setContentText(state.detail)
            .setOnlyAlertOnce(true)
            .setContentIntent(open)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)

        if (state.running && state.total > 0) {
            builder.setProgress(state.total, state.done, false)
        } else if (state.running) {
            builder.setProgress(0, 0, true)
        }

        if (state.running) {
            val stop = PendingIntent.getService(
                this,
                1,
                Intent(this, SyncService::class.java).setAction(ACTION_STOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            builder.addAction(0, getString(R.string.stop_sync), stop)
            builder.setOngoing(true)
        }

        return builder.build()
    }

    private val notificationManager
        get() = getSystemService(Context.NOTIFICATION_SERVICE) as android.app.NotificationManager

    companion object {
        const val ACTION_STOP = "dev.phonesync.app.STOP"
        private const val NOTIFICATION_ID = 41

        fun start(context: Context) {
            val intent = Intent(context, SyncService::class.java)
            androidx.core.content.ContextCompat.startForegroundService(context, intent)
        }
    }
}
