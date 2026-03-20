package com.quailsync.app.data

import android.content.Context
import android.util.Log
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.time.Duration
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit
import java.util.concurrent.TimeUnit

class HatchCountdownWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {

    companion object {
        private const val TAG = "QuailSync-Hatch"
        private const val WORK_NAME = "hatch_countdown_daily"
        private const val INCUBATION_DAYS = 17L

        fun schedule(context: Context) {
            // Calculate delay until 8am
            val now = LocalDateTime.now()
            val target = if (now.toLocalTime().isBefore(LocalTime.of(8, 0))) {
                now.toLocalDate().atTime(8, 0)
            } else {
                now.toLocalDate().plusDays(1).atTime(8, 0)
            }
            val delayMinutes = ChronoUnit.MINUTES.between(now, target)

            val request = PeriodicWorkRequestBuilder<HatchCountdownWorker>(
                1, TimeUnit.DAYS,
            ).setInitialDelay(delayMinutes, TimeUnit.MINUTES)
                .build()

            WorkManager.getInstance(context).enqueueUniquePeriodicWork(
                WORK_NAME,
                ExistingPeriodicWorkPolicy.KEEP,
                request,
            )
            Log.d(TAG, "Hatch countdown worker scheduled, initial delay: ${delayMinutes}min")
        }
    }

    override suspend fun doWork(): Result {
        Log.d(TAG, "Running hatch countdown check")
        return try {
            val api = QuailSyncApi.create()
            val clutches = api.getClutches()
            val today = LocalDate.now()

            for (clutch in clutches) {
                val status = clutch.status?.lowercase() ?: continue
                if (status !in listOf("incubating", "active", "set")) continue

                val setDate = clutch.setDate?.let { parseDate(it) } ?: continue
                val daysElapsed = ChronoUnit.DAYS.between(setDate, today).toInt()
                val daysUntilHatch = INCUBATION_DAYS.toInt() - daysElapsed
                val clutchLabel = clutch.bloodlineName ?: "Clutch #${clutch.id}"

                when (daysUntilHatch) {
                    in Int.MIN_VALUE..0 -> {
                        NotificationHelper.fireHatchNotification(
                            applicationContext,
                            "Hatch day!",
                            "$clutchLabel is at day $daysElapsed — check for hatching chicks!",
                            clutch.id,
                        )
                    }
                    1 -> {
                        NotificationHelper.fireHatchNotification(
                            applicationContext,
                            "1 day until hatch!",
                            "$clutchLabel hatches tomorrow",
                            clutch.id,
                        )
                    }
                    3 -> {
                        NotificationHelper.fireHatchNotification(
                            applicationContext,
                            "3 days until hatch — Lockdown",
                            "$clutchLabel enters lockdown today (day 14). Stop turning eggs.",
                            clutch.id,
                        )
                    }
                    10 -> {
                        NotificationHelper.fireHatchNotification(
                            applicationContext,
                            "7 days — Time to candle",
                            "$clutchLabel is at day 7. Candle eggs to check fertility.",
                            clutch.id,
                        )
                    }
                }
            }

            Log.d(TAG, "Hatch countdown check complete, checked ${clutches.size} clutches")
            Result.success()
        } catch (e: Exception) {
            Log.e(TAG, "Hatch countdown check failed", e)
            Result.retry()
        }
    }

    private fun parseDate(dateStr: String): LocalDate? {
        return try {
            LocalDate.parse(dateStr, DateTimeFormatter.ISO_LOCAL_DATE)
        } catch (_: Exception) {
            try {
                LocalDate.parse(dateStr.take(10), DateTimeFormatter.ISO_LOCAL_DATE)
            } catch (_: Exception) { null }
        }
    }
}
