package app.ropeway

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat

/**
 * Shows the phone screen to other devices: MediaProjection draws into a
 * hardware H.264 encoder, whose output goes to Rust. Runs as a foreground
 * service, as Android requires for screen capture; its notification has a
 * Stop button.
 */
class CaptureService : Service() {
  private var projection: MediaProjection? = null
  private var display: VirtualDisplay? = null
  private var codec: MediaCodec? = null
  private var thread: HandlerThread? = null
  private var config: ByteArray? = null
  @Volatile private var stopping = false

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == ACTION_STOP) {
      stopSelf()
      return START_NOT_STICKY
    }
    if (intent == null || projection != null) return START_NOT_STICKY
    // The foreground service must run before the projection is created.
    goForeground()
    try {
      start(intent)
    } catch (e: Exception) {
      Log.e(RopewayBridge.TAG, "starting capture", e)
      stopSelf()
    }
    return START_NOT_STICKY
  }

  private fun start(intent: Intent) {
    val code = intent.getIntExtra(EXTRA_CODE, 0)
    val data: Intent = if (Build.VERSION.SDK_INT >= 33) {
      intent.getParcelableExtra(EXTRA_DATA, Intent::class.java)
    } else {
      @Suppress("DEPRECATION") intent.getParcelableExtra(EXTRA_DATA)
    } ?: throw IllegalStateException("no consent")
    val width = intent.getIntExtra(EXTRA_WIDTH, 720)
    val height = intent.getIntExtra(EXTRA_HEIGHT, 1600)
    val fps = intent.getIntExtra(EXTRA_FPS, 30)
    val bitrate = intent.getIntExtra(EXTRA_BITRATE, 4_000_000)
    captureWidth = width
    captureHeight = height

    val handlerThread = HandlerThread("ropeway-capture").apply { start() }
    thread = handlerThread
    val handler = Handler(handlerThread.looper)

    val mpm = getSystemService(MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
    val proj = mpm.getMediaProjection(code, data) ?: throw IllegalStateException("no projection")
    projection = proj
    proj.registerCallback(object : MediaProjection.Callback() {
      override fun onStop() {
        stopSelf()
      }
    }, handler)

    val format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, width, height).apply {
      setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
      setInteger(MediaFormat.KEY_BIT_RATE, bitrate)
      setInteger(MediaFormat.KEY_FRAME_RATE, fps)
      setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 3)
      // A still screen produces no frames; repeating the last one keeps the
      // stream alive, so new viewers get a picture.
      setLong(MediaFormat.KEY_REPEAT_PREVIOUS_FRAME_AFTER, 250_000L)
    }
    val enc = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
    codec = enc
    enc.setCallback(object : MediaCodec.Callback() {
      override fun onInputBufferAvailable(c: MediaCodec, index: Int) {}

      override fun onOutputBufferAvailable(c: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
        if (stopping) return
        try {
          val buf = c.getOutputBuffer(index)
          if (buf != null && info.size > 0) {
            val bytes = ByteArray(info.size)
            buf.position(info.offset)
            buf.get(bytes)
            if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
              config = bytes
            } else {
              val key = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
              // Viewers configure their decoder from a keyframe, so each one
              // carries the SPS/PPS.
              val out = if (key && config != null) config!! + bytes else bytes
              RopewayNative.onVideo(out, key, info.presentationTimeUs)
            }
          }
          c.releaseOutputBuffer(index, false)
        } catch (e: IllegalStateException) {
          // Codec stopped meanwhile.
        }
      }

      override fun onError(c: MediaCodec, e: MediaCodec.CodecException) {
        Log.e(RopewayBridge.TAG, "encoder", e)
        stopSelf()
      }

      override fun onOutputFormatChanged(c: MediaCodec, format: MediaFormat) {}
    }, handler)
    enc.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
    val surface = enc.createInputSurface()
    enc.start()

    val dpi = resources.displayMetrics.densityDpi
    display = proj.createVirtualDisplay(
      "Ropeway", width, height, dpi,
      DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, surface, null, handler,
    )
    instance = this
    RopewayNative.onCaptureSize(width, height)
  }

  fun requestKeyframe() {
    try {
      codec?.setParameters(Bundle().apply { putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0) })
    } catch (e: IllegalStateException) {
      // Not running.
    }
  }

  private fun goForeground() {
    val nm = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
    if (Build.VERSION.SDK_INT >= 26) {
      nm.createNotificationChannel(
        NotificationChannel(CHANNEL, getString(R.string.capture_channel), NotificationManager.IMPORTANCE_LOW),
      )
    }
    val stop = PendingIntent.getService(
      this, 0, Intent(this, CaptureService::class.java).setAction(ACTION_STOP),
      PendingIntent.FLAG_IMMUTABLE,
    )
    val open = PendingIntent.getActivity(
      this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
      PendingIntent.FLAG_IMMUTABLE,
    )
    val notification = NotificationCompat.Builder(this, CHANNEL)
      .setSmallIcon(R.drawable.ic_stat_ropeway)
      .setContentTitle(getString(R.string.capture_title))
      .setContentText(getString(R.string.capture_text))
      .setContentIntent(open)
      .setOngoing(true)
      .addAction(0, getString(R.string.capture_stop), stop)
      .build()
    if (Build.VERSION.SDK_INT >= 29) {
      startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION)
    } else {
      startForeground(NOTIFICATION_ID, notification)
    }
  }

  override fun onDestroy() {
    stopping = true
    instance = null
    try { display?.release() } catch (_: Exception) {}
    try { codec?.stop() } catch (_: Exception) {}
    try { codec?.release() } catch (_: Exception) {}
    try { projection?.stop() } catch (_: Exception) {}
    thread?.quitSafely()
    display = null
    codec = null
    projection = null
    RopewayNative.onCaptureStopped()
    super.onDestroy()
  }

  companion object {
    @Volatile var instance: CaptureService? = null
    /** Size of the picture being sent, for mapping remote input. */
    @Volatile var captureWidth = 0
    @Volatile var captureHeight = 0
    const val CHANNEL = "ropeway-capture"
    const val NOTIFICATION_ID = 4710
    const val ACTION_STOP = "app.ropeway.STOP_CAPTURE"
    const val EXTRA_CODE = "code"
    const val EXTRA_DATA = "data"
    const val EXTRA_WIDTH = "width"
    const val EXTRA_HEIGHT = "height"
    const val EXTRA_FPS = "fps"
    const val EXTRA_BITRATE = "bitrate"
  }
}
