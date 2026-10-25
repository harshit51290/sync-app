package dev.phonesync.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit

/// Background runs never start a foreground service from the background
/// (Android 15 forbids that for dataSync); the worker itself does the upload.
class SyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        val prefs = App.prefs
        if (!prefs.isPaired() || !prefs.autoSync) return Result.success()
        val summary = SyncEngine.sync(applicationContext, "auto")
        return if (summary.failed > 0 && summary.sent == 0) Result.retry() else Result.success()
    }
}

object SyncScheduler {

    private const val AUTO = "phonesync-auto"

    fun ensure(context: Context) {
        val prefs = Prefs(context)
        val workManager = WorkManager.getInstance(context)
        if (!prefs.autoSync || !prefs.isPaired()) {
            workManager.cancelUniqueWork(AUTO)
            return
        }
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(
                if (prefs.wifiOnly) NetworkType.UNMETERED else NetworkType.CONNECTED,
            )
            .build()
        val request = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES)
            .setConstraints(constraints)
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 5, TimeUnit.MINUTES)
            .build()
        workManager.enqueueUniquePeriodicWork(AUTO, ExistingPeriodicWorkPolicy.UPDATE, request)
    }
}

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action == Intent.ACTION_BOOT_COMPLETED) {
            SyncScheduler.ensure(context)
        }
    }
}
