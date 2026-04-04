package com.quailsync.app.data

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import com.quailsync.app.MainActivity

object NotificationHelper {
    const val CHANNEL_ALERTS = "quailsync_alerts"
    const val CHANNEL_HATCH = "quailsync_hatch"
    const val CHANNEL_MONITOR = "quailsync_monitor"

    const val MONITOR_NOTIFICATION_ID = 1
    private var nextAlertId = 100
    private var nextHatchId = 200

    fun createChannels(context: Context) {
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

        val alertChannel = NotificationChannel(
            CHANNEL_ALERTS,
            "Temperature Alerts",
            NotificationManager.IMPORTANCE_HIGH,
        ).apply {
            description = "Alerts when brooder temperature is outside age-based thresholds"
            enableVibration(true)
        }

        val hatchChannel = NotificationChannel(
            CHANNEL_HATCH,
            "Hatch Countdowns",
            NotificationManager.IMPORTANCE_DEFAULT,
        ).apply {
            description = "Notifications for upcoming hatch dates and milestones"
        }

        val monitorChannel = NotificationChannel(
            CHANNEL_MONITOR,
            "Background Monitoring",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Persistent notification while QuailSync is monitoring brooders"
            setShowBadge(false)
        }

        manager.createNotificationChannels(listOf(alertChannel, hatchChannel, monitorChannel))
    }

    fun buildMonitorNotification(context: Context, brooderCount: Int): NotificationCompat.Builder {
        val intent = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val pending = PendingIntent.getActivity(
            context, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        val text = if (brooderCount > 0) {
            "Monitoring $brooderCount brooder${if (brooderCount != 1) "s" else ""}"
        } else {
            "Connecting to server..."
        }

        return NotificationCompat.Builder(context, CHANNEL_MONITOR)
            .setSmallIcon(android.R.drawable.ic_menu_compass)
            .setContentTitle("QuailSync monitoring")
            .setContentText(text)
            .setOngoing(true)
            .setContentIntent(pending)
            .setSilent(true)
    }

    fun fireAlertNotification(
        context: Context,
        brooderName: String,
        message: String,
        severity: String,
        brooderId: Int,
    ) {
        val intent = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val pending = PendingIntent.getActivity(
            context, brooderId, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        val priority = if (severity == "CRITICAL") {
            NotificationCompat.PRIORITY_HIGH
        } else {
            NotificationCompat.PRIORITY_DEFAULT
        }

        val notification = NotificationCompat.Builder(context, CHANNEL_ALERTS)
            .setSmallIcon(android.R.drawable.ic_dialog_alert)
            .setContentTitle("$severity: $brooderName")
            .setContentText(message)
            .setPriority(priority)
            .setContentIntent(pending)
            .setAutoCancel(true)
            .build()

        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        // Use brooder-specific ID so each brooder gets its own notification
        manager.notify(MONITOR_NOTIFICATION_ID + 10 + brooderId, notification)
    }

    fun fireHatchNotification(context: Context, title: String, message: String, clutchId: Int) {
        val intent = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val pending = PendingIntent.getActivity(
            context, clutchId + 1000, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        val notification = NotificationCompat.Builder(context, CHANNEL_HATCH)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle(title)
            .setContentText(message)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setContentIntent(pending)
            .setAutoCancel(true)
            .build()

        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.notify(nextHatchId++, notification)
    }
}
