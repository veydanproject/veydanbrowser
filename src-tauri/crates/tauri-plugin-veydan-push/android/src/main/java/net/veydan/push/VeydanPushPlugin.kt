// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

package net.veydan.push

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.os.Build
import android.util.Log
import android.webkit.WebView
import androidx.lifecycle.ProcessLifecycleOwner
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
import com.google.firebase.FirebaseApp
import com.google.firebase.messaging.FirebaseMessaging

@InvokeArg
class ContextArgs {
  var live: Boolean = false
}

@InvokeArg
class CancelArgs {
  var key: String? = null
}

private const val NOTIFICATIONS = "notifications"

/** The app's side of the push bridge. Called from the app's Rust code only. */
@TauriPlugin(
  permissions = [
    Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = NOTIFICATIONS)
  ]
)
class VeydanPushPlugin(private val activity: Activity) : Plugin(activity) {
  override fun load(webView: WebView) {
    PushState.attach(this)
    Notifier.ensureChannels(activity)
    activity.runOnUiThread {
      ProcessLifecycleOwner.get().lifecycle.addObserver(PushState.screen)
    }
    // A tap may be what started the app.
    Notifier.readTap(activity.intent)?.let { PushState.putTap(it) }
  }

  override fun onNewIntent(intent: Intent) {
    val tap = Notifier.readTap(intent) ?: return
    PushState.putTap(tap)
    trigger("tap", JSObject())
  }

  internal fun tokenChanged(token: String) {
    trigger("token", JSObject().put("token", token))
  }

  /** Why the phone cannot receive pushes, or null when it can. */
  private fun unavailable(): String? {
    if (FirebaseApp.getApps(activity).isEmpty()) return "no_firebase_config"
    val play = GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(activity)
    if (play != ConnectionResult.SUCCESS) return "no_play_services"
    return null
  }

  /** What is known without a word to the push service. */
  @Command
  fun getState(invoke: Invoke) {
    val reason = unavailable()
    invoke.resolve(
      JSObject()
        .put("available", reason == null)
        .put("reason", reason)
        .put("permission", state())
        .put("appId", activity.packageName)
    )
  }

  @Command
  fun getToken(invoke: Invoke) {
    val answer = JSObject().put("appId", activity.packageName)
    val reason = unavailable()
    if (reason != null) {
      invoke.resolve(answer.put("available", false).put("reason", reason))
      return
    }
    val messaging = FirebaseMessaging.getInstance()
    // From now on the service may renew the token by itself.
    messaging.isAutoInitEnabled = true
    messaging.token.addOnCompleteListener { task ->
      if (task.isSuccessful && !task.result.isNullOrEmpty()) {
        // For the one who debugs with the phone on a cable:
        //   adb shell setprop log.tag.VeydanPush VERBOSE
        if (Log.isLoggable(PushState.TAG, Log.VERBOSE)) {
          Log.v(PushState.TAG, "token=${task.result}")
        }
        invoke.resolve(answer.put("available", true).put("token", task.result))
      } else {
        invoke.resolve(
          answer.put("available", false)
            .put("reason", "token_failed")
            .put("detail", task.exception?.message ?: "")
        )
      }
    }
  }

  @Command
  fun deleteToken(invoke: Invoke) {
    if (unavailable() != null) {
      invoke.resolve()
      return
    }
    val messaging = FirebaseMessaging.getInstance()
    messaging.isAutoInitEnabled = false
    messaging.deleteToken().addOnCompleteListener { task ->
      if (task.isSuccessful) invoke.resolve()
      else invoke.reject(task.exception?.message ?: "the token could not be given up")
    }
  }

  private fun state(): String {
    if (Notifier.allowed(activity)) return PermissionState.GRANTED.toString()
    // Before Android 13 there is nothing to ask: the user turned them off.
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
      return PermissionState.DENIED.toString()
    }
    return when (val state = getPermissionState(NOTIFICATIONS)) {
      // Granted by the system, turned off in the app's settings.
      PermissionState.GRANTED, null -> PermissionState.DENIED.toString()
      else -> state.toString()
    }
  }

  @Command
  fun permissionState(invoke: Invoke) {
    invoke.resolve(JSObject().put("state", state()))
  }

  @Command
  fun requestPermission(invoke: Invoke) {
    val asked = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      getPermissionState(NOTIFICATIONS) != PermissionState.GRANTED
    if (asked) {
      requestPermissionForAlias(NOTIFICATIONS, invoke, "permissionAnswered")
    } else {
      permissionState(invoke)
    }
  }

  @PermissionCallback
  fun permissionAnswered(invoke: Invoke) {
    permissionState(invoke)
  }

  @Command
  fun setContext(invoke: Invoke) {
    PushState.live = invoke.parseArgs(ContextArgs::class.java).live
    invoke.resolve()
  }

  @Command
  fun takeTap(invoke: Invoke) {
    val answer = JSObject()
    PushState.takeTap()?.let { tap ->
      answer.put("tap", JSObject().put("type", tap.type).put("chat", tap.chat))
    }
    invoke.resolve(answer)
  }

  @Command
  fun cancel(invoke: Invoke) {
    val key = invoke.parseArgs(CancelArgs::class.java).key
    if (key != null && !Push.isChatKey(key)) {
      invoke.reject("not a chat: $key")
      return
    }
    Notifier.cancel(activity, key)
    invoke.resolve()
  }
}
