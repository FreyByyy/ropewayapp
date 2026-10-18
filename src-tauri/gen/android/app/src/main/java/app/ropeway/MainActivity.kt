package app.ropeway

import android.Manifest
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  private lateinit var bridge: RopewayBridge

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    bridge = RopewayBridge(this)
    bridge.start()

    // Edge to edge: keep the page clear of the status bar, the navigation
    // bar and the keyboard.
    val root = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.ime())
      v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }

    askPermissions()
  }

  override fun onDestroy() {
    bridge.stop()
    super.onDestroy()
  }

  @Deprecated("Deprecated in Java")
  override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
    @Suppress("DEPRECATION")
    super.onActivityResult(requestCode, resultCode, data)
    bridge.onActivityResult(requestCode, resultCode, data)
  }

  /**
   * Notifications for new messages, and access to shared storage: received
   * files go to Download/Ropeway, the shared folder is Documents/Ropeway Shared.
   */
  private fun askPermissions() {
    val wanted = mutableListOf<String>()
    if (Build.VERSION.SDK_INT >= 33 &&
      ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
    ) {
      wanted.add(Manifest.permission.POST_NOTIFICATIONS)
    }
    if (Build.VERSION.SDK_INT <= 29 &&
      ContextCompat.checkSelfPermission(this, Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED
    ) {
      wanted.add(Manifest.permission.WRITE_EXTERNAL_STORAGE)
    }
    if (wanted.isNotEmpty()) ActivityCompat.requestPermissions(this, wanted.toTypedArray(), 1)

    if (Build.VERSION.SDK_INT >= 30 && !Environment.isExternalStorageManager()) {
      AlertDialog.Builder(this)
        .setTitle(R.string.files_title)
        .setMessage(R.string.files_text)
        .setPositiveButton(R.string.files_allow) { _, _ ->
          val intent = Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:$packageName"))
          try {
            startActivity(intent)
          } catch (e: Exception) {
            startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
          }
        }
        .setNegativeButton(R.string.files_later, null)
        .show()
    }
  }
}
