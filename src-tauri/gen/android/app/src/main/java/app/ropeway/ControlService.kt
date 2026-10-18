package app.ropeway

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.graphics.Path
import android.os.Build
import android.os.Bundle
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo

/**
 * Remote control of the phone from a computer, through Android's
 * accessibility service: the only way an app may tap, swipe and type into
 * other apps without root. The user turns it on once in the system settings.
 *
 * Coordinates arrive relative to the shared picture (0..1); the picture is
 * the screen scaled into the capture size, so they are mapped back here.
 */
class ControlService : AccessibilityService() {
  override fun onServiceConnected() {
    instance = this
  }

  override fun onUnbind(intent: android.content.Intent?): Boolean {
    instance = null
    return super.onUnbind(intent)
  }

  override fun onAccessibilityEvent(event: AccessibilityEvent?) {}

  override fun onInterrupt() {}

  /** Screen pixel for a point of the shared picture. */
  private fun toScreen(x: Float, y: Float): Pair<Float, Float> {
    val (sw, sh) = realSize()
    val cw = CaptureService.captureWidth.takeIf { it > 0 } ?: sw
    val ch = CaptureService.captureHeight.takeIf { it > 0 } ?: sh
    // The capture shows the screen scaled to fit, centered.
    val scale = minOf(cw.toFloat() / sw, ch.toFloat() / sh)
    val offX = (cw - sw * scale) / 2f
    val offY = (ch - sh * scale) / 2f
    val px = ((x * cw) - offX) / scale
    val py = ((y * ch) - offY) / scale
    return px.coerceIn(0f, sw - 1f) to py.coerceIn(0f, sh - 1f)
  }

  private fun realSize(): Pair<Int, Int> {
    val wm = getSystemService(WINDOW_SERVICE) as android.view.WindowManager
    return if (Build.VERSION.SDK_INT >= 30) {
      val b = wm.currentWindowMetrics.bounds
      b.width() to b.height()
    } else {
      val m = android.util.DisplayMetrics()
      @Suppress("DEPRECATION") wm.defaultDisplay.getRealMetrics(m)
      m.widthPixels to m.heightPixels
    }
  }

  /** A press from one point to another: a tap, a long press or a swipe. */
  fun stroke(x1: Float, y1: Float, x2: Float, y2: Float, durationMs: Long) {
    val (ax, ay) = toScreen(x1, y1)
    val (bx, by) = toScreen(x2, y2)
    val path = Path().apply {
      moveTo(ax, ay)
      lineTo(bx, by)
    }
    val gesture = GestureDescription.Builder()
      .addStroke(GestureDescription.StrokeDescription(path, 0, durationMs.coerceIn(1, 10_000)))
      .build()
    dispatchGesture(gesture, null, null)
  }

  /** Scrolls the content under a point: positive `notches` scroll down. */
  fun scroll(x: Float, y: Float, notches: Float) {
    val (_, sh) = realSize()
    val (px, py) = toScreen(x, y)
    val distance = (notches * sh / 12f).coerceIn(-sh * 0.6f, sh * 0.6f)
    val from = py.coerceIn(sh * 0.2f, sh * 0.8f)
    val to = (from - distance).coerceIn(1f, sh - 1f)
    val path = Path().apply {
      moveTo(px, from)
      lineTo(px, to)
    }
    val gesture = GestureDescription.Builder()
      .addStroke(GestureDescription.StrokeDescription(path, 0, 180))
      .build()
    dispatchGesture(gesture, null, null)
  }

  fun back() = performGlobalAction(GLOBAL_ACTION_BACK)

  fun home() = performGlobalAction(GLOBAL_ACTION_HOME)

  fun recents() = performGlobalAction(GLOBAL_ACTION_RECENTS)

  private fun focusedField(): AccessibilityNodeInfo? =
    rootInActiveWindow?.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)?.takeIf { it.isEditable }

  /** Types into the focused text field. */
  fun type(text: String) {
    val node = focusedField() ?: return
    val current = node.text?.toString().takeUnless { node.isShowingHintTextCompat() } ?: ""
    val start = node.textSelectionStart.takeIf { it >= 0 } ?: current.length
    val end = node.textSelectionEnd.takeIf { it >= 0 } ?: current.length
    val (a, b) = minOf(start, end).coerceAtMost(current.length) to maxOf(start, end).coerceAtMost(current.length)
    val next = current.substring(0, a) + text + current.substring(b)
    setText(node, next, a + text.length)
  }

  /** Erases the character before the cursor, or the selection. */
  fun backspace() {
    val node = focusedField() ?: return
    val current = node.text?.toString().takeUnless { node.isShowingHintTextCompat() } ?: return
    val start = node.textSelectionStart.takeIf { it >= 0 } ?: current.length
    val end = node.textSelectionEnd.takeIf { it >= 0 } ?: current.length
    val (a, b) = minOf(start, end).coerceAtMost(current.length) to maxOf(start, end).coerceAtMost(current.length)
    if (a == b && a == 0) return
    val from = if (a == b) a - 1 else a
    setText(node, current.substring(0, from) + current.substring(b), from)
  }

  fun enter() {
    val node = focusedField() ?: return
    if (Build.VERSION.SDK_INT >= 30) {
      node.performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id)
    } else {
      type("\n")
    }
  }

  private fun setText(node: AccessibilityNodeInfo, text: String, cursor: Int) {
    node.performAction(
      AccessibilityNodeInfo.ACTION_SET_TEXT,
      Bundle().apply { putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text) },
    )
    node.performAction(
      AccessibilityNodeInfo.ACTION_SET_SELECTION,
      Bundle().apply {
        putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, cursor)
        putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, cursor)
      },
    )
  }

  private fun AccessibilityNodeInfo.isShowingHintTextCompat(): Boolean =
    Build.VERSION.SDK_INT >= 26 && isShowingHintText

  companion object {
    @Volatile var instance: ControlService? = null
  }
}
