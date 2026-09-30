// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

package net.veydan.push

import org.json.JSONObject

/** What the messenger's core made of a push (messenger-notify, `Outcome`). */
internal sealed class Outcome {
  /** A message to show in full: who, what, where. */
  data class Show(val notice: Notice) : Outcome()

  /** Something came; no more can or may be said. */
  data class Plain(val plain: PlainNotice) : Outcome()

  /** Nothing to show, and why. */
  data class Quiet(val reason: String) : Outcome()

  /** The core could not answer. */
  data class Error(val error: String) : Outcome()

  companion object {
    fun from(json: JSONObject): Outcome = when (json.optString("outcome")) {
      "show" -> Show(Notice.from(json))
      "plain" -> Plain(PlainNotice.from(json))
      "quiet" -> Quiet(json.optString("reason", "?"))
      else -> Error(json.optString("error", "unknown outcome"))
    }
  }
}

/** What a message was, without a word of any language. */
internal sealed class Body {
  data class Text(val text: String) : Body()
  data class Media(val kind: String, val name: String, val caption: String?) : Body()
  data class Invite(val groupName: String) : Body()
  data class JoinRequest(val groupName: String) : Body()
  data class Welcome(val groupName: String) : Body()

  companion object {
    fun from(json: JSONObject?): Body? {
      json ?: return null
      return when (json.optString("t")) {
        "text" -> Text(json.optString("text"))
        "media" -> Media(json.optString("kind"), json.optString("name"), json.optString("caption").takeIf { it.isNotEmpty() })
        "invite" -> Invite(json.optString("group_name"))
        "join_request" -> JoinRequest(json.optString("group_name"))
        "welcome" -> Welcome(json.optString("group_name"))
        else -> null
      }
    }
  }
}

internal data class Notice(
  /** `dm` | `request` | `group` */
  val kind: String,
  val chat: String?,
  val title: String,
  val sender: String,
  val senderKey: String,
  val body: Body?,
  val muted: Boolean,
  val hideOnLockscreen: Boolean,
  val count: Int,
) {
  companion object {
    fun from(json: JSONObject) = Notice(
      kind = json.optString("kind", "dm"),
      chat = json.optString("chat").takeIf { Push.isChatKey(it) },
      title = json.optString("title"),
      sender = json.optString("sender"),
      senderKey = json.optString("sender_key"),
      body = Body.from(json.optJSONObject("body")),
      muted = json.optBoolean("muted", false),
      hideOnLockscreen = json.optBoolean("hide_on_lockscreen", false),
      count = json.optInt("count", 1).coerceIn(1, 9999),
    )
  }
}

internal data class PlainNotice(
  /** `dm` | `group` */
  val kind: String,
  val chat: String?,
  /** The group's name, when known. */
  val title: String?,
  val muted: Boolean,
  val count: Int,
) {
  companion object {
    fun from(json: JSONObject) = PlainNotice(
      kind = json.optString("kind", "dm"),
      chat = json.optString("chat").takeIf { Push.isChatKey(it) },
      title = json.optString("title").takeIf { it.isNotEmpty() },
      muted = json.optBoolean("muted", false),
      count = json.optInt("count", 1).coerceIn(1, 9999),
    )

    /** What is known of a push without the core: its kind and its group. */
    fun of(push: Push) = PlainNotice(
      kind = push.type,
      chat = push.groupId?.let { "group:$it" },
      title = null,
      muted = false,
      count = push.count,
    )
  }
}
