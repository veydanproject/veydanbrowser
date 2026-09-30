// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

package net.veydan.push

import android.content.Context
import android.util.Log
import org.json.JSONObject

/**
 * The messenger's own reading of a push: the app's Rust library, loaded
 * without the app. It opens the event with the keys and reads the
 * messenger's database, and says what the notification is about.
 *
 * Loading the library starts nothing: its entry points wait for a window.
 * A build without the messenger has no such function at all, and that is
 * an error of linking, not an exception; both come back as null here.
 */
internal object Core {
  private val loaded: Boolean by lazy {
    try {
      System.loadLibrary("veydan_lib")
      true
    } catch (e: Throwable) {
      Log.w(PushState.TAG, "the app's library did not load: ${e.javaClass.simpleName}")
      false
    }
  }

  /**
   * The outcome as JSON (`outcome`: `show` | `plain` | `quiet`), or null
   * when the library could not answer. `dataDir` is the messenger's
   * directory; `bundle` is the opened key bundle; `data` is the push's
   * data map as a JSON object.
   */
  fun describe(context: Context, bundle: ByteArray, data: Map<String, String>): JSONObject? {
    if (!loaded) return null
    val dataDir = context.dataDir.resolve("messenger").absolutePath
    val push = JSONObject(data as Map<*, *>).toString()
    val answer = try {
      describe(dataDir, bundle, push)
    } catch (e: Throwable) {
      Log.w(PushState.TAG, "the core did not answer: ${e.javaClass.simpleName}")
      null
    } ?: return null
    return try {
      JSONObject(answer)
    } catch (e: Exception) {
      Log.w(PushState.TAG, "the core's answer is not JSON")
      null
    }
  }

  @JvmStatic
  private external fun describe(dataDir: String, bundle: ByteArray, push: String): String?
}
