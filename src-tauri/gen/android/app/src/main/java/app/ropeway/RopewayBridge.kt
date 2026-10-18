package app.ropeway

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.media.projection.MediaProjectionManager
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.provider.OpenableColumns
import android.util.Log
import android.webkit.MimeTypeMap
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import androidx.core.view.WindowCompat
import org.json.JSONArray
import java.io.File

/**
 * What Rust calls on the Android side. Methods may be called from any thread;
 * anything that touches the UI is moved to the main thread.
 */
class RopewayBridge(private val activity: MainActivity) {
  private val app: Context = activity.applicationContext
  private var multicastLock: WifiManager.MulticastLock? = null

  /** Consent for the next capture: result code and data of the system dialog. */
  @Volatile private var consent: Pair<Int, Intent>? = null
  private var captureRequest = 0L
  private var pickRequest = 0L

  fun start() {
    // Many phones drop broadcast packets unless a multicast lock is held;
    // discovery of other devices depends on them.
    val wifi = app.getSystemService(Context.WIFI_SERVICE) as WifiManager
    multicastLock = wifi.createMulticastLock("ropeway-discovery").apply {
      setReferenceCounted(false)
      acquire()
    }
    RopewayNative.attach(this)
  }

  fun stop() {
    multicastLock?.release()
  }

  // ---- screen capture ----

  fun requestCapture(id: Long) {
    activity.runOnUiThread {
      captureRequest = id
      val mpm = app.getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
      try {
        activity.startActivityForResult(mpm.createScreenCaptureIntent(), REQ_CAPTURE)
      } catch (e: Exception) {
        Log.w(TAG, "screen capture dialog", e)
        RopewayNative.onResult(id, null)
      }
    }
  }

  /** Starts capture with the consent given last; false if there is none. */
  fun startCapture(width: Int, height: Int, fps: Int, bitrate: Int): Boolean {
    val (code, data) = consent ?: return false
    consent = null
    val intent = Intent(app, CaptureService::class.java)
      .putExtra(CaptureService.EXTRA_CODE, code)
      .putExtra(CaptureService.EXTRA_DATA, data)
      .putExtra(CaptureService.EXTRA_WIDTH, width)
      .putExtra(CaptureService.EXTRA_HEIGHT, height)
      .putExtra(CaptureService.EXTRA_FPS, fps)
      .putExtra(CaptureService.EXTRA_BITRATE, bitrate)
    ContextCompat.startForegroundService(app, intent)
    return true
  }

  fun stopCapture() {
    app.stopService(Intent(app, CaptureService::class.java))
  }

  fun requestKeyframe() {
    CaptureService.instance?.requestKeyframe()
  }

  /** Physical screen size in pixels, as "width,height". */
  fun screenSize(): String {
    val wm = activity.windowManager
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      val b = wm.maximumWindowMetrics.bounds
      "${b.width()},${b.height()}"
    } else {
      val m = android.util.DisplayMetrics()
      @Suppress("DEPRECATION") wm.defaultDisplay.getRealMetrics(m)
      "${m.widthPixels},${m.heightPixels}"
    }
  }

  // ---- remote control ----

  private var pointer = 0.5f to 0.5f
  private var pressed: Triple<Float, Float, Long>? = null

  /** Whether the user has turned on Ropeway in the accessibility settings. */
  fun controlEnabled(): Boolean = ControlService.instance != null

  fun openControlSettings() {
    activity.runOnUiThread {
      activity.startActivity(Intent(android.provider.Settings.ACTION_ACCESSIBILITY_SETTINGS))
    }
  }

  /**
   * Input from a computer controlling the phone (an `InputEvent` as JSON).
   * Left button: tap, long press or swipe from where it went down to where
   * it came up. Right button: Back. Middle: Home. Wheel: swipe-scroll.
   */
  fun input(json: String) {
    val svc = ControlService.instance ?: return
    val e = try { org.json.JSONObject(json) } catch (_: Exception) { return }
    when (e.optString("t")) {
      "mouse_move" -> pointer = e.optDouble("x").toFloat() to e.optDouble("y").toFloat()
      "mouse_button" -> {
        val down = e.optBoolean("down")
        when (e.optInt("button")) {
          0 -> if (down) {
            pressed = Triple(pointer.first, pointer.second, System.currentTimeMillis())
          } else {
            val p = pressed ?: return
            pressed = null
            val held = System.currentTimeMillis() - p.third
            svc.stroke(p.first, p.second, pointer.first, pointer.second, held.coerceIn(40, 3000))
          }
          1 -> if (!down) svc.home()
          2 -> if (!down) svc.back()
          3 -> if (!down) svc.back()
        }
      }
      "wheel" -> svc.scroll(pointer.first, pointer.second, e.optDouble("dy").toFloat())
      "text" -> svc.type(e.optString("text"))
      "key" -> if (e.optBoolean("down")) {
        when (e.optString("code")) {
          "Backspace" -> svc.backspace()
          "Enter", "NumpadEnter" -> svc.enter()
          "Escape", "BrowserBack" -> svc.back()
          "MetaLeft", "MetaRight", "Home", "BrowserHome" -> svc.home()
          "ContextMenu" -> svc.recents()
        }
      }
      "release_all" -> pressed = null
    }
  }

  // ---- files ----

  fun pickFiles(id: Long) {
    activity.runOnUiThread {
      pickRequest = id
      val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
        .addCategory(Intent.CATEGORY_OPENABLE)
        .setType("*/*")
        .putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
      try {
        activity.startActivityForResult(intent, REQ_PICK)
      } catch (e: ActivityNotFoundException) {
        RopewayNative.onResult(id, null)
      }
    }
  }

  /** Opens a file in another app. Returns an error message, or null. */
  fun openFile(path: String): String? {
    val file = File(path)
    if (!file.exists()) return "file not found"
    val uri = FileProvider.getUriForFile(app, "${app.packageName}.fileprovider", file)
    val ext = file.extension.lowercase()
    val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "*/*"
    val intent = Intent(Intent.ACTION_VIEW)
      .setDataAndType(uri, mime)
      .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    return try {
      activity.startActivity(Intent.createChooser(intent, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
      null
    } catch (e: ActivityNotFoundException) {
      "no app can open this file"
    }
  }

  // ---- system bars ----

  /** Colors the status and navigation bar areas like the app's background. */
  fun setSystemBars(color: String, dark: Boolean) {
    activity.runOnUiThread {
      val parsed = try { Color.parseColor(color) } catch (e: IllegalArgumentException) { return@runOnUiThread }
      activity.window.decorView.setBackgroundColor(parsed)
      val controller = WindowCompat.getInsetsController(activity.window, activity.window.decorView)
      controller.isAppearanceLightStatusBars = !dark
      controller.isAppearanceLightNavigationBars = !dark
    }
  }

  // ---- activity results ----

  fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
    when (requestCode) {
      REQ_CAPTURE -> {
        if (resultCode == Activity.RESULT_OK && data != null) {
          consent = resultCode to data
          RopewayNative.onResult(captureRequest, "ok")
        } else {
          RopewayNative.onResult(captureRequest, null)
        }
      }
      REQ_PICK -> {
        val id = pickRequest
        val uris = mutableListOf<Uri>()
        data?.clipData?.let { clip -> for (i in 0 until clip.itemCount) uris.add(clip.getItemAt(i).uri) }
        if (uris.isEmpty()) data?.data?.let { uris.add(it) }
        if (resultCode != Activity.RESULT_OK || uris.isEmpty()) {
          RopewayNative.onResult(id, null)
          return
        }
        // Copying may take a while; the transfer code needs plain files.
        Thread {
          val paths = JSONArray()
          val dir = File(app.cacheDir, "outgoing/${System.currentTimeMillis()}").apply { mkdirs() }
          for (uri in uris) {
            try {
              val target = File(dir, displayName(uri))
              app.contentResolver.openInputStream(uri)?.use { input ->
                target.outputStream().use { input.copyTo(it) }
              }
              paths.put(target.absolutePath)
            } catch (e: Exception) {
              Log.w(TAG, "copying $uri", e)
            }
          }
          RopewayNative.onResult(id, paths.toString())
        }.start()
      }
    }
  }

  private fun displayName(uri: Uri): String {
    app.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
      if (c.moveToFirst()) {
        val name = c.getString(0)
        if (!name.isNullOrBlank()) return name.replace('/', '_')
      }
    }
    return uri.lastPathSegment?.substringAfterLast('/') ?: "file"
  }

  companion object {
    const val TAG = "Ropeway"
    const val REQ_CAPTURE = 4701
    const val REQ_PICK = 4702
  }
}
