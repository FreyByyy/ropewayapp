package app.ropeway

/** Functions implemented in Rust (src-tauri/src/android.rs). */
object RopewayNative {
  /** Hands Rust the bridge it calls into. */
  external fun attach(bridge: RopewayBridge)

  /** Answer to a request Rust made with [id]; `null` means cancelled. */
  external fun onResult(id: Long, value: String?)

  /** One encoded H.264 access unit (Annex B). */
  external fun onVideo(data: ByteArray, keyframe: Boolean, ptsUs: Long)

  /** Capture ended on the phone. */
  external fun onCaptureStopped()

  external fun onCaptureSize(width: Int, height: Int)
}
