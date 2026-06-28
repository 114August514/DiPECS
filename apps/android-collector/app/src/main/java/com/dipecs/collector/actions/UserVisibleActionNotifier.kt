package com.dipecs.collector.actions

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import com.dipecs.collector.MainActivity
import com.dipecs.collector.R
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

object UserVisibleActionNotifier {
    fun postLaunchHint(context: Context, target: String?, reason: String): Boolean {
        val appContext = context.applicationContext
        val manager = appContext.getSystemService(NotificationManager::class.java)
        if (manager == null) {
            EventRepository.recordInternal(
                appContext,
                "user_visible_action_failed",
                "NotificationManager unavailable",
                JSONObject()
                    .put("target", target ?: JSONObject.NULL)
                    .put("reason", reason),
            )
            return false
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N && !manager.areNotificationsEnabled()) {
            EventRepository.recordInternal(
                appContext,
                "user_visible_action_skipped",
                "Notifications are disabled",
                JSONObject()
                    .put("target", target ?: JSONObject.NULL)
                    .put("reason", reason),
            )
            return false
        }

        createChannel(manager)
        manager.notify(NOTIFICATION_ID, notification(appContext))
        EventRepository.recordInternal(
            appContext,
            "user_visible_action_posted",
            "Posted user-visible action hint",
            JSONObject()
                .put("target", target ?: JSONObject.NULL)
                .put("reason", reason),
        )
        return true
    }

    private fun notification(context: Context): Notification {
        val intent = Intent(context, MainActivity::class.java)
        val pendingIntent = PendingIntent.getActivity(
            context,
            0,
            intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(context, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(context)
        }
        return builder
            .setSmallIcon(android.R.drawable.stat_notify_more)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText("Open DiPECS to review the suggested action")
            .setContentIntent(pendingIntent)
            .setAutoCancel(true)
            .build()
    }

    private fun createChannel(manager: NotificationManager) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return
        }
        val channel = NotificationChannel(
            CHANNEL_ID,
            "User-visible actions",
            NotificationManager.IMPORTANCE_DEFAULT,
        )
        channel.description = "Shows DiPECS action hints that require user visibility"
        manager.createNotificationChannel(channel)
    }

    private const val CHANNEL_ID = "dipecs_action_hints"
    private const val NOTIFICATION_ID = 1201
}
