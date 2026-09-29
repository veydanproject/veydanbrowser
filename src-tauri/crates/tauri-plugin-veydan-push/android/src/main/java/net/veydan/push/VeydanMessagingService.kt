// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

package net.veydan.push

import android.util.Log
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

/**
 * Receives pushes. The system starts it on its own, also when the app is not
 * running, so nothing here may count on the app's window or on Rust.
 *
 * Every push leaves one line in the log with its trace id, the same id the
 * server logs: what came and what was done with it, never what it said.
 */
class VeydanMessagingService : FirebaseMessagingService() {
  override fun onMessageReceived(message: RemoteMessage) {
    val push = Push.from(message.data)
    if (push == null) {
      Log.w(PushState.TAG, "push dropped: no type")
      return
    }
    Log.i(PushState.TAG, "push type=${push.type} trace=${push.trace}: ${handle(push)}")
  }

  private fun handle(push: Push): String {
    if (push.silent) {
      // Service pushes without text; their handling comes with the features
      // that need them. Nothing is shown, which is what silent means.
      return "silent"
    }
    // The user is looking at the app and the app gets messages by itself:
    // the message is already on the screen.
    if (push.aboutMessage && PushState.visible && PushState.live) {
      return "not shown, the app is on the screen"
    }
    return if (Notifier.show(this, push)) "shown" else "not shown, notifications are off"
  }

  override fun onNewToken(token: String) {
    Log.i(PushState.TAG, "the push service gave a new token")
    PushState.plugin()?.tokenChanged(token)
  }
}
