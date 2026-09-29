// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

package net.veydan.push

import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import java.lang.ref.WeakReference

/**
 * What the push service and the plugin share while the process lives.
 *
 * All of it starts empty in a process the push service started by itself:
 * nobody is looking, nothing is live, so every push is shown.
 */
internal object PushState {
  const val TAG = "VeydanPush"

  /** The app is on the screen. Set by the plugin, which exists only with the app's window. */
  @Volatile
  var visible: Boolean = false
    private set

  /** The app receives messages by itself right now. Told by the app. */
  @Volatile
  var live: Boolean = false

  /** The tap nobody has asked about yet. */
  private var tap: Tap? = null

  private var plugin: WeakReference<VeydanPushPlugin>? = null

  val screen = object : DefaultLifecycleObserver {
    override fun onStart(owner: LifecycleOwner) {
      visible = true
    }

    override fun onStop(owner: LifecycleOwner) {
      visible = false
    }
  }

  @Synchronized
  fun attach(plugin: VeydanPushPlugin) {
    this.plugin = WeakReference(plugin)
  }

  @Synchronized
  fun plugin(): VeydanPushPlugin? = plugin?.get()

  @Synchronized
  fun putTap(tap: Tap) {
    this.tap = tap
  }

  @Synchronized
  fun takeTap(): Tap? = tap.also { tap = null }
}

internal data class Tap(val type: String, val chat: String?)
