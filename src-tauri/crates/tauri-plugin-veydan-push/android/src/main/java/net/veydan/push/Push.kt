// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

package net.veydan.push

/**
 * One push, as the server sent it (VPush/spec/protocol.md). Everything in it
 * comes from outside and is treated so: what does not look right is dropped.
 */
internal data class Push(
  val version: Int,
  val type: String,
  val title: String?,
  val body: String?,
  val groupId: String?,
  val count: Int?,
  val trace: String?,
) {
  /** A push without text is handled, not shown. */
  val silent: Boolean
    get() = title.isNullOrBlank() && body.isNullOrBlank()

  /** About a message, as opposed to a word from the server itself. */
  val aboutMessage: Boolean
    get() = type == TYPE_DM || type == TYPE_GROUP

  /** Notifications with the same key replace one another. */
  val key: String
    get() = when (type) {
      TYPE_DM -> KEY_DM
      TYPE_GROUP -> "$KEY_GROUP${groupId ?: ""}"
      else -> "service:${trace ?: type}"
    }

  /** The chat a tap opens. A direct message names none: the server does not know the sender. */
  val chat: String?
    get() = if (type == TYPE_GROUP && groupId != null) "$KEY_GROUP$groupId" else null

  companion object {
    /** The version of the format this code understands. */
    const val VERSION = 1

    const val TYPE_DM = "dm"
    const val TYPE_GROUP = "group"
    const val KEY_DM = "dm"
    const val KEY_GROUP = "group:"

    private val TYPE = Regex("^[a-z_]{1,32}$")
    private val HEX64 = Regex("^[0-9a-f]{64}$")
    private val TRACE = Regex("^[0-9A-Za-z_-]{1,64}$")

    fun from(data: Map<String, String>): Push? {
      val type = data["type"]?.takeIf { TYPE.matches(it) } ?: return null
      return Push(
        version = data["v"]?.toIntOrNull() ?: 1,
        type = type,
        title = data["title"]?.take(200),
        body = data["body"]?.take(1000),
        groupId = data["group_id"]?.takeIf { HEX64.matches(it) },
        count = data["count"]?.toIntOrNull()?.takeIf { it in 2..9999 },
        trace = data["trace"]?.takeIf { TRACE.matches(it) },
      )
    }

    /** Is this a key the app may ask to clear. */
    fun isChatKey(key: String): Boolean =
      key == KEY_DM || (key.startsWith(KEY_GROUP) && HEX64.matches(key.removePrefix(KEY_GROUP)))
  }
}
