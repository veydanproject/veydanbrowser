// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

package net.veydan.push

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

/** Shows and removes notifications. Needs no window and no Rust. */
internal object Notifier {
  private const val CHANNEL_DM = "dm"
  private const val CHANNEL_GROUPS = "groups"
  private const val CHANNEL_SERVICE = "service"

  const val EXTRA_TYPE = "veydan_push_type"
  const val EXTRA_CHAT = "veydan_push_chat"

  /** Notifications are told apart by their tag; the number is the same for all. */
  private const val ID = 1

  /** Channels are what the user sees in the system settings, one switch each. */
  fun ensureChannels(context: Context) {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    manager.createNotificationChannels(
      listOf(
        NotificationChannel(
          CHANNEL_DM,
          context.getString(R.string.veydan_push_channel_dm),
          NotificationManager.IMPORTANCE_HIGH,
        ),
        NotificationChannel(
          CHANNEL_GROUPS,
          context.getString(R.string.veydan_push_channel_groups),
          NotificationManager.IMPORTANCE_HIGH,
        ),
        NotificationChannel(
          CHANNEL_SERVICE,
          context.getString(R.string.veydan_push_channel_service),
          NotificationManager.IMPORTANCE_DEFAULT,
        ),
      )
    )
  }

  fun allowed(context: Context): Boolean =
    NotificationManagerCompat.from(context).areNotificationsEnabled()

  /** False when the system would not show it. */
  fun show(context: Context, push: Push): Boolean {
    if (!allowed(context)) return false
    ensureChannels(context)

    val newer = push.version > Push.VERSION
    val title = if (newer) context.getString(R.string.veydan_push_update_title) else push.title
    val body = if (newer) context.getString(R.string.veydan_push_update_body) else push.body

    val channel = when (push.type) {
      Push.TYPE_DM -> CHANNEL_DM
      Push.TYPE_GROUP -> CHANNEL_GROUPS
      else -> CHANNEL_SERVICE
    }
    val builder = NotificationCompat.Builder(context, channel)
      .setSmallIcon(R.drawable.ic_stat_veydan)
      .setContentTitle(title)
      .setContentText(body)
      .setAutoCancel(true)
      .setOnlyAlertOnce(false)
      .setCategory(
        if (push.aboutMessage) NotificationCompat.CATEGORY_MESSAGE
        else NotificationCompat.CATEGORY_STATUS
      )
    if (!body.isNullOrEmpty()) {
      builder.setStyle(NotificationCompat.BigTextStyle().bigText(body))
    }
    push.count?.let { builder.setNumber(it) }
    open(context, push)?.let { builder.setContentIntent(it) }

    return try {
      NotificationManagerCompat.from(context).notify(push.key, ID, builder.build())
      true
    } catch (e: SecurityException) {
      // The permission was taken away between the check and the call.
      false
    }
  }

  /** The app's own start, with a note of what was tapped. */
  private fun open(context: Context, push: Push): PendingIntent? {
    val intent = context.packageManager.getLaunchIntentForPackage(context.packageName)
      ?: return null
    intent.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
    intent.putExtra(EXTRA_TYPE, push.type)
    push.chat?.let { intent.putExtra(EXTRA_CHAT, it) }
    return PendingIntent.getActivity(
      context,
      push.key.hashCode(),
      intent,
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )
  }

  /** Removes the notification of one chat, or every notification about messages. */
  fun cancel(context: Context, key: String?) {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    if (key != null) {
      manager.cancel(key, ID)
      return
    }
    for (shown in manager.activeNotifications) {
      val tag = shown.tag ?: continue
      if (Push.isChatKey(tag)) manager.cancel(tag, shown.id)
    }
  }

  /** What a tap carried, if the intent is one of ours. The note is removed: it is read once. */
  fun readTap(intent: Intent?): Tap? {
    val type = intent?.getStringExtra(EXTRA_TYPE) ?: return null
    val chat = intent.getStringExtra(EXTRA_CHAT)?.takeIf { Push.isChatKey(it) }
    intent.removeExtra(EXTRA_TYPE)
    intent.removeExtra(EXTRA_CHAT)
    return Tap(type.take(32), chat)
  }
}
