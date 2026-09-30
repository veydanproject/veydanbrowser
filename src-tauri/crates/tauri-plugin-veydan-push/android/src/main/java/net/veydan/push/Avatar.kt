// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

package net.veydan.push

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect

/**
 * The circle with initials the app shows for people without a picture,
 * drawn the same way here: two letters, a hue from the key. Pictures are
 * addresses on the network, and a notification is not worth a download.
 */
internal object Avatar {
  private const val SIZE = 108

  fun of(label: String, seed: String): Bitmap {
    val bitmap = Bitmap.createBitmap(SIZE, SIZE, Bitmap.Config.ARGB_8888)
    val canvas = Canvas(bitmap)
    val hue = hue(seed.ifEmpty { label })
    val tone = Color.HSVToColor(floatArrayOf(hue, 0.55f, 0.72f))
    val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    paint.color = tone
    canvas.drawCircle(SIZE / 2f, SIZE / 2f, SIZE / 2f, paint)

    val text = initials(label)
    paint.color = Color.WHITE
    paint.textSize = SIZE * 0.42f
    paint.textAlign = Paint.Align.CENTER
    paint.isFakeBoldText = true
    val bounds = Rect()
    paint.getTextBounds(text, 0, text.length, bounds)
    canvas.drawText(text, SIZE / 2f, SIZE / 2f - bounds.exactCenterY(), paint)
    return bitmap
  }

  /** As the app: first letters of two words, or two of one; keys get a glyph. */
  fun initials(label: String): String {
    val s = label.trim()
    if (s.isEmpty()) return "?"
    if (s.startsWith("npub1", ignoreCase = true) || Regex("^[0-9a-f]{16,}", RegexOption.IGNORE_CASE).containsMatchIn(s)) return "#"
    val words = s.split(Regex("\\s+")).filter { it.isNotEmpty() }
    val pick = if (words.size > 1) first(words[0]) + first(words[1]) else words[0].codePoints().limit(2).toArray().let { String(it, 0, it.size) }
    return pick.uppercase()
  }

  private fun first(word: String): String {
    val cp = word.codePointAt(0)
    return String(Character.toChars(cp))
  }

  /** The app's hash of the seed, so the colour matches the chat list. */
  private fun hue(seed: String): Float {
    var h = 0L
    for (ch in seed) h = (h * 31 + ch.code) and 0xffffffffL
    return (h % 360).toFloat()
  }
}
