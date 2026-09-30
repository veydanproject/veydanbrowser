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
import androidx.core.app.Person
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat

/**
 * Shows and removes notifications. Needs no window and no Rust: the words
 * are this phone's own strings, the facts come from the messenger's core
 * or, without it, from the push alone.
 *
 * One notification per chat, tagged with the chat; a new message joins the
 * ones already shown. All of them hang under one summary, so that a tap on
 * the summary opens the messenger too.
 */
internal object Notifier {
  private const val CHANNEL_DM = "dm"
  private const val CHANNEL_GROUPS = "groups"
  private const val CHANNEL_SERVICE = "service"

  const val EXTRA_TYPE = "veydan_push_type"
  const val EXTRA_CHAT = "veydan_push_chat"

  /** Notifications are told apart by their tag; the number is the same for all. */
  private const val ID = 1
  private const val GROUP = "net.veydan.messages"
  private const val TAG_SUMMARY = "summary"
  /** A direct message the phone could not open is one for all of them. */
  private const val TAG_DM_PLAIN = "dm"
  private const val MAX_LINES = 25

  /** Channels are what the user sees in the system settings, one switch each. */
  fun ensureChannels(context: Context) {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    manager.createNotificationChannels(
      listOf(
        NotificationChannel(CHANNEL_DM, context.getString(R.string.veydan_push_channel_dm), NotificationManager.IMPORTANCE_HIGH),
        NotificationChannel(CHANNEL_GROUPS, context.getString(R.string.veydan_push_channel_groups), NotificationManager.IMPORTANCE_HIGH),
        NotificationChannel(CHANNEL_SERVICE, context.getString(R.string.veydan_push_channel_service), NotificationManager.IMPORTANCE_DEFAULT),
      )
    )
  }

  fun allowed(context: Context): Boolean =
    NotificationManagerCompat.from(context).areNotificationsEnabled()

  /** A message with everything known about it. False when the system would not show it. */
  fun show(context: Context, notice: Notice): Boolean {
    if (!allowed(context)) return false
    ensureChannels(context)

    val tag = notice.chat ?: TAG_DM_PLAIN
    val group = notice.kind == "group"
    val face = IconCompat.createWithBitmap(Avatar.of(context, notice.sender, notice.senderKey, notice.picture))
    val sender = Person.Builder()
      .setName(notice.sender)
      .setKey(notice.senderKey)
      .setIcon(face)
      .build()
    val me = Person.Builder().setName(context.getString(R.string.veydan_push_me)).build()

    // The lines already on the screen for this chat stay; the new one joins them.
    val style = shown(context, tag)?.let { NotificationCompat.MessagingStyle.extractMessagingStyleFromNotification(it) }
      ?: NotificationCompat.MessagingStyle(me)
    style.setGroupConversation(group)
    if (group) style.setConversationTitle(notice.title)
    style.addMessage(line(context, notice), System.currentTimeMillis(), sender)
    while (style.messages.size > MAX_LINES) style.messages.removeAt(0)

    val builder = NotificationCompat.Builder(context, if (group) CHANNEL_GROUPS else CHANNEL_DM)
      .setSmallIcon(R.drawable.ic_stat_veydan)
      .setStyle(style)
      .setCategory(NotificationCompat.CATEGORY_MESSAGE)
      .setGroup(GROUP)
      .setAutoCancel(true)
      .setSilent(notice.muted)
      .setContentIntent(open(context, if (group) Push.TYPE_GROUP else Push.TYPE_DM, notice.chat))
    // A chat of its own in the system's eyes: its face becomes the
    // notification's, and it goes to the "Conversations" section.
    notice.chat?.let { chat ->
      val icon = if (group) IconCompat.createWithBitmap(Avatar.initials(notice.title, chat)) else face
      if (conversation(context, chat, notice.title, if (group) null else sender, icon, group)) builder.setShortcutId(chat)
    }
    if (notice.hideOnLockscreen) {
      builder
        .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
        .setPublicVersion(plainBuilder(context, PlainNotice(notice.kind, notice.chat, null, notice.muted, 1)).build())
    }
    return post(context, tag, builder) && summary(context)
  }

  /**
   * The chat as a long-lived shortcut: what Android wants before it shows a
   * notification as a conversation, with the chat's face instead of the
   * app's. False when the system would not take it.
   */
  private fun conversation(context: Context, chat: String, title: String, person: Person?, icon: IconCompat, group: Boolean): Boolean {
    val intent = context.packageManager.getLaunchIntentForPackage(context.packageName) ?: return false
    intent.putExtra(EXTRA_TYPE, if (group) Push.TYPE_GROUP else Push.TYPE_DM)
    intent.putExtra(EXTRA_CHAT, chat)
    val shortcut = ShortcutInfoCompat.Builder(context, chat)
      .setShortLabel(title.ifBlank { "Veydan" })
      .setLongLived(true)
      .setIcon(icon)
      .setIntent(intent)
      .apply { person?.let { setPerson(it) } }
      .build()
    return try {
      ShortcutManagerCompat.pushDynamicShortcut(context, shortcut)
      true
    } catch (e: Exception) {
      false
    }
  }

  /** Something came, and that is all: the phone has no key, or may say no more. */
  fun showPlain(context: Context, plain: PlainNotice): Boolean {
    if (!allowed(context)) return false
    ensureChannels(context)
    val tag = plain.chat ?: TAG_DM_PLAIN
    return post(context, tag, plainBuilder(context, plain)) && summary(context)
  }

  /** A word from the server itself: a test, a broadcast. */
  fun showService(context: Context, push: Push): Boolean {
    if (!allowed(context)) return false
    ensureChannels(context)
    val newer = push.version > Push.VERSION
    val (title, body) = when {
      newer -> context.getString(R.string.veydan_push_update_title) to context.getString(R.string.veydan_push_update_body)
      push.type == Push.TYPE_TEST -> context.getString(R.string.veydan_push_test_title) to context.getString(R.string.veydan_push_test_body)
      else -> context.getString(R.string.veydan_push_service_title) to (push.data ?: "")
    }
    val builder = NotificationCompat.Builder(context, CHANNEL_SERVICE)
      .setSmallIcon(R.drawable.ic_stat_veydan)
      .setContentTitle(title)
      .setContentText(body)
      .setStyle(NotificationCompat.BigTextStyle().bigText(body))
      .setCategory(NotificationCompat.CATEGORY_STATUS)
      .setAutoCancel(true)
      .setContentIntent(open(context, push.type, null))
    return post(context, "service:${push.trace}", builder)
  }

  private fun plainBuilder(context: Context, plain: PlainNotice): NotificationCompat.Builder {
    val group = plain.kind == "group"
    val title = when {
      group -> plain.title ?: context.getString(R.string.veydan_push_group)
      else -> context.getString(R.string.veydan_push_dm_title)
    }
    val body = if (plain.count > 1) context.getString(R.string.veydan_push_many, plain.count)
      else context.getString(R.string.veydan_push_one)
    return NotificationCompat.Builder(context, if (group) CHANNEL_GROUPS else CHANNEL_DM)
      .setSmallIcon(R.drawable.ic_stat_veydan)
      .setContentTitle(title)
      .setContentText(body)
      .setNumber(plain.count)
      .setCategory(NotificationCompat.CATEGORY_MESSAGE)
      .setGroup(GROUP)
      .setAutoCancel(true)
      .setSilent(plain.muted)
      .setContentIntent(open(context, if (group) Push.TYPE_GROUP else Push.TYPE_DM, plain.chat))
  }

  /** The one line of a message, in this phone's words. */
  private fun line(context: Context, notice: Notice): String {
    val more = if (notice.count > 1) " " + context.getString(R.string.veydan_push_more, notice.count - 1) else ""
    val body = notice.body ?: return context.getString(R.string.veydan_push_one) + more
    val text = when (body) {
      is Body.Text -> body.text
      is Body.Media -> body.caption ?: when (body.kind) {
        "image" -> context.getString(R.string.veydan_push_media_image)
        "video" -> context.getString(R.string.veydan_push_media_video)
        "audio" -> context.getString(R.string.veydan_push_media_audio)
        "voice" -> context.getString(R.string.veydan_push_media_voice)
        "circle" -> context.getString(R.string.veydan_push_media_circle)
        else -> context.getString(R.string.veydan_push_media_file, body.name)
      }
      is Body.Invite -> context.getString(R.string.veydan_push_invite, body.groupName)
      is Body.JoinRequest -> context.getString(R.string.veydan_push_join_request, body.groupName)
      is Body.Welcome -> context.getString(R.string.veydan_push_welcome, body.groupName)
    }
    val request = if (notice.kind == "request") context.getString(R.string.veydan_push_request) + " · " else ""
    return request + text + more
  }

  private fun shown(context: Context, tag: String): android.app.Notification? {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return null
    return manager.activeNotifications.firstOrNull { it.tag == tag && it.id == ID }?.notification
  }

  /** The one notification the group of them collapses into. */
  private fun summary(context: Context): Boolean {
    val builder = NotificationCompat.Builder(context, CHANNEL_DM)
      .setSmallIcon(R.drawable.ic_stat_veydan)
      .setGroup(GROUP)
      .setGroupSummary(true)
      // The summary never makes a sound: each chat decides for itself. A
      // muted chat's notification hands its alert to the summary, and a
      // summary that took it would ring for a chat the user silenced.
      .setGroupAlertBehavior(NotificationCompat.GROUP_ALERT_CHILDREN)
      .setOnlyAlertOnce(true)
      .setCategory(NotificationCompat.CATEGORY_MESSAGE)
      .setAutoCancel(true)
      .setContentIntent(open(context, Push.TYPE_DM, null))
    return post(context, TAG_SUMMARY, builder)
  }

  private fun post(context: Context, tag: String, builder: NotificationCompat.Builder): Boolean = try {
    NotificationManagerCompat.from(context).notify(tag, ID, builder.build())
    true
  } catch (e: SecurityException) {
    // The permission was taken away between the check and the call.
    false
  }

  /** The app's own start, with a note of what was tapped. */
  private fun open(context: Context, type: String, chat: String?): PendingIntent? {
    val intent = context.packageManager.getLaunchIntentForPackage(context.packageName) ?: return null
    intent.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
    intent.putExtra(EXTRA_TYPE, type)
    chat?.let { intent.putExtra(EXTRA_CHAT, it) }
    return PendingIntent.getActivity(
      context,
      (chat ?: type).hashCode(),
      intent,
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )
  }

  /** A tag the app may ask to clear: a chat, or the one for direct messages it could not tell apart. */
  fun isClearable(key: String): Boolean = Push.isChatKey(key) || key == TAG_DM_PLAIN

  /**
   * Removes the notification of one chat (`dm:<pubkey>`, `group:<id>`), or
   * every notification about messages. The summary goes when nothing is
   * left under it.
   */
  fun cancel(context: Context, key: String?) {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    if (key != null) {
      manager.cancel(key, ID)
      // A direct chat opened is also the place a message the phone could
      // not open was about.
      if (key.startsWith("dm:")) manager.cancel(TAG_DM_PLAIN, ID)
    } else {
      for (shown in manager.activeNotifications) {
        val tag = shown.tag ?: continue
        if (Push.isChatKey(tag) || tag == TAG_DM_PLAIN) manager.cancel(tag, shown.id)
      }
    }
    val left = manager.activeNotifications.any { it.tag != null && it.tag != TAG_SUMMARY && (Push.isChatKey(it.tag) || it.tag == TAG_DM_PLAIN) }
    if (!left) manager.cancel(TAG_SUMMARY, ID)
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
