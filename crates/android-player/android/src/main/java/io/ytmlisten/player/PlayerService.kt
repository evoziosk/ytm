package io.ytmlisten.player

import android.app.*
import android.content.*
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaMetadata
import android.media.VolumeProvider
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.*
import android.webkit.*
import androidx.core.content.ContextCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject
import org.json.JSONObject

/** The service owns the YouTube session independently of the activity and never extracts audio URLs. */
class PlayerService: Service() {
    companion object {
        var instance: PlayerService? = null
        var bridge = ""
        var channel: Channel? = null
        fun allowed(value: String): Boolean {
            val u = Uri.parse(value)
            val host = u.host ?: return false
            return u.scheme == "https" && (u.port == -1 || u.port == 443) && u.userInfo == null && (host == "music.youtube.com" || host == "www.youtube.com" || host == "youtube.com" || host == "accounts.google.com" || host == "consent.google.com" || host == "myaccount.google.com")
        }
    }
    lateinit var player: WebView
    private lateinit var media: MediaSession
    private lateinit var audio: AudioManager
    private lateinit var focus: AudioFocusRequest
    private lateinit var wake: PowerManager.WakeLock
    private var playing = false
    private var focused = false
    private var title = "YTM Listen"
    private var artist = "Ready to play"
    private var remoteActive = false
    private var remoteTarget = ""
    private var remoteMessage = ""
    private var remoteTransport = false
    private var remoteSkip = false
    private var remoteSeek = false
    private var multicast: WifiManager.MulticastLock? = null
    private var multicastUsers = 0
    fun discoveryMulticast(enabled: Boolean) {
        if (enabled) {
            if (multicast == null) multicast = (applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager).createMulticastLock("ytmlisten:discovery").apply { setReferenceCounted(false) }
            if (multicast?.isHeld != true) multicast?.acquire()
            multicastUsers++
        } else {
            multicastUsers = (multicastUsers - 1).coerceAtLeast(0)
            if (multicastUsers == 0 && multicast?.isHeld == true) multicast?.release()
        }
    }
    private val noisy = object: BroadcastReceiver() { override fun onReceive(context: Context, intent: Intent) { if (!remoteActive) command("pause") } }
    fun remoteState(status: JSONObject) {
        val wasRemote = remoteActive
        remoteActive = status.optBoolean("active")
        remoteTarget = if (remoteActive) status.optString("target") else ""
        remoteMessage = status.optString("message")
        if (remoteActive) {
            if (wake.isHeld) wake.release()
            if (focused) { audio.abandonAudioFocusRequest(focus); focused = false }
            val snapshot = status.optJSONObject("snapshot")
            val participant = status.optJSONObject("participant")
            val ready = snapshot != null && participant?.optBoolean("approved") == true && status.optString("message") == "Connected"
            val canVolume = ready && participant?.optJSONObject("permissions")?.optBoolean("volume") == true
            remoteTransport = ready && participant?.optJSONObject("permissions")?.optBoolean("transport") == true
            remoteSkip = ready && participant?.optJSONObject("permissions")?.optBoolean("skip") == true
            remoteSeek = ready && participant?.optJSONObject("permissions")?.optBoolean("seek") == true
            val level = ((snapshot?.optDouble("volume", 0.0) ?: 0.0) * 100).toInt().coerceIn(0, 100)
            media.setPlaybackToRemote(object: VolumeProvider(if (canVolume) VolumeProvider.VOLUME_CONTROL_ABSOLUTE else VolumeProvider.VOLUME_CONTROL_FIXED, 100, level) {
                override fun onSetVolumeTo(volume: Int) { if (canVolume) channel?.send(JSObject().put("kind", "command").put("payload", JSObject().put("type", "volume").put("volume", volume.coerceIn(0, 100) / 100.0))) }
                override fun onAdjustVolume(direction: Int) { onSetVolumeTo(currentVolume + direction * 5) }
            })
            if (snapshot != null) {
                update(snapshot)
                var actions = 0L
                if (remoteTransport) actions = actions or PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE
                if (remoteSkip) actions = actions or PlaybackState.ACTION_SKIP_TO_NEXT or PlaybackState.ACTION_SKIP_TO_PREVIOUS
                if (remoteSeek) actions = actions or PlaybackState.ACTION_SEEK_TO
                media.setPlaybackState(PlaybackState.Builder().setActions(actions).setState(if (playing) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED, (snapshot.optDouble("position", 0.0).coerceIn(0.0, 604800.0) * 1000).toLong(), if (playing) 1f else 0f).build())
            }
            if (!ready) {
                playing = false
                media.setPlaybackState(PlaybackState.Builder().setActions(0).setState(PlaybackState.STATE_PAUSED, 0, 0f).build())
                getSystemService(NotificationManager::class.java).notify(7341, notification())
            }
        } else if (wasRemote) {
            media.setPlaybackToLocal(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
            playing = false
            title = "YTM Listen"; artist = "Ready to play"
            media.setPlaybackState(PlaybackState.Builder().setActions(0).setState(PlaybackState.STATE_PAUSED, 0, 0f).build())
            getSystemService(NotificationManager::class.java).notify(7341, notification())
        }
    }
    private fun command(type: String, position: Long? = null) {
        if (remoteActive && when(type) {
            "play", "pause" -> !remoteTransport
            "next", "previous" -> !remoteSkip
            "seek" -> !remoteSeek
            else -> true
        }) return
        val payload = JSObject().put("type", type)
        if (position != null) payload.put("position", position / 1000.0)
        channel?.send(JSObject().put("kind", "command").put("payload", payload))
    }
    override fun onCreate() {
        super.onCreate(); instance = this
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel("playback", "Music playback", NotificationManager.IMPORTANCE_LOW))
        media = MediaSession(this, "YTM Listen")
        media.setCallback(object: MediaSession.Callback() {
            override fun onPlay() { command("play") }
            override fun onPause() { command("pause") }
            override fun onSkipToNext() { command("next") }
            override fun onSkipToPrevious() { command("previous") }
            override fun onSeekTo(pos: Long) { command("seek", pos) }
            override fun onStop() { command("pause") }
        })
        media.isActive = true
        audio = getSystemService(AudioManager::class.java)
        focus = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
            .setOnAudioFocusChangeListener { change -> if (change < 0) { focused = false; command("pause") } }
            .build()
        wake = getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "ytmlisten:playback")
        ContextCompat.registerReceiver(this, noisy, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), ContextCompat.RECEIVER_NOT_EXPORTED)
        startForeground(7341, notification())
        player = WebView(this)
        player.settings.javaScriptEnabled = true
        player.settings.domStorageEnabled = true
        player.settings.mediaPlaybackRequiresUserGesture = false
        player.settings.allowFileAccess = false
        player.settings.allowContentAccess = false
        player.settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
        // Desktop Music exposes the same playback/catalog interface used by the desktop bridge.
        player.settings.userAgentString = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"
        CookieManager.getInstance().setAcceptCookie(true)
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
            channel?.send(JSObject().put("kind", "error").put("payload", "Update Android System WebView to enable the playback bridge")); return
        }
        WebViewCompat.addWebMessageListener(player, "__YTM_ANDROID__", setOf("https://music.youtube.com")) { _, message, origin, mainFrame, _ ->
            if (!mainFrame || origin.toString() != "https://music.youtube.com" || (message.data?.length ?: 0) > 4_000_000) return@addWebMessageListener
            try {
                val body = JSONObject(message.data!!)
                val kind = body.optString("command")
                val args = body.optJSONObject("args") ?: return@addWebMessageListener
                if (kind == "player_event") {
                    val event = args.getJSONObject("event")
                    channel?.send(JSObject().put("kind", kind).put("payload", event))
                    if (!remoteActive) event.optJSONObject("state")?.let { update(it) }
                } else if (kind == "catalog_result") channel?.send(JSObject().put("kind", kind).put("payload", args.getJSONObject("reply")))
            } catch (_: Exception) { /* Ignore malformed page messages. */ }
        }
        val adapter = "window.__YTM_ANDROID_INVOKE__=(command,args)=>{window.__YTM_ANDROID__.postMessage(JSON.stringify({command,args}));return Promise.resolve();};"
        val injection = adapter + bridge
        if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) WebViewCompat.addDocumentStartJavaScript(player, injection, setOf("https://music.youtube.com"))
        player.webViewClient = object: WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean = !allowed(request.url.toString())
            override fun onPageFinished(view: WebView, url: String) {
                if (Uri.parse(url).host == "music.youtube.com") view.evaluateJavascript(injection, null)
                CookieManager.getInstance().flush()
            }
        }
        player.loadUrl("https://music.youtube.com")
    }
    private fun update(state: JSONObject) {
        val nextPlaying = state.optBoolean("playing")
        if (nextPlaying && !focused && !remoteActive) {
            focused = audio.requestAudioFocus(focus) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
            if (!focused) command("pause")
        }
        if (nextPlaying && !wake.isHeld && !remoteActive) wake.acquire(60 * 60 * 1000L)
        if ((!nextPlaying || remoteActive) && wake.isHeld) wake.release()
        if ((!nextPlaying || remoteActive) && focused) {audio.abandonAudioFocusRequest(focus);focused=false}
        playing = nextPlaying
        val track = state.optJSONObject("track")
        title = track?.optString("title") ?: "YTM Listen"
        artist = track?.optString("artist") ?: "Ready to play"
        val duration = (state.optDouble("duration", 0.0).coerceIn(0.0,604800.0)*1000).toLong()
        media.setMetadata(MediaMetadata.Builder().putString(MediaMetadata.METADATA_KEY_TITLE,title).putString(MediaMetadata.METADATA_KEY_ARTIST,artist).putLong(MediaMetadata.METADATA_KEY_DURATION,duration).build())
        media.setPlaybackState(PlaybackState.Builder().setActions(PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_SKIP_TO_NEXT or PlaybackState.ACTION_SKIP_TO_PREVIOUS or PlaybackState.ACTION_SEEK_TO or PlaybackState.ACTION_PLAY_PAUSE).setState(if(playing)PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,(state.optDouble("position",0.0).coerceIn(0.0,604800.0)*1000).toLong(),if(playing)1f else 0f).build())
        getSystemService(NotificationManager::class.java).notify(7341,notification())
    }
    private fun pending(action: String): PendingIntent = PendingIntent.getService(this, action.hashCode(), Intent(this,PlayerService::class.java).setAction(action), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    private fun notification(): Notification {
        val open = packageManager.getLaunchIntentForPackage(packageName)
        val style = Notification.MediaStyle().setMediaSession(media.sessionToken)
        val text = if (remoteActive) {
            if (remoteMessage == "Connected") "$artist - Playing on $remoteTarget" else "$remoteTarget - $remoteMessage"
        } else artist
        val builder = Notification.Builder(this,"playback").setSmallIcon(android.R.drawable.ic_media_play).setContentTitle(title).setContentText(text).setVisibility(Notification.VISIBILITY_PUBLIC).setOngoing(playing).setStyle(style)
        var count = 0
        if (!remoteActive || remoteSkip) {
            builder.addAction(Notification.Action.Builder(android.R.drawable.ic_media_previous,"Previous",pending("previous")).build())
            count++
        }
        if (!remoteActive || remoteTransport) {
            builder.addAction(Notification.Action.Builder(if(playing)android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play,if(playing)"Pause" else "Play",pending(if(playing)"pause" else "play")).build())
            count++
        }
        if (!remoteActive || remoteSkip) {
            builder.addAction(Notification.Action.Builder(android.R.drawable.ic_media_next,"Next",pending("next")).build())
            count++
        }
        if (count > 0) style.setShowActionsInCompactView(*IntArray(count) { it })
        if(open!=null)builder.setContentIntent(PendingIntent.getActivity(this,0,open,PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
        return builder.build()
    }
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if(intent?.action in listOf("play","pause","next","previous"))command(intent!!.action!!)
        return START_NOT_STICKY // Process loss creates a new session; never silently adopt old guests.
    }
    override fun onBind(intent: Intent?): IBinder? = null
    override fun onDestroy() {
        if (!remoteActive) command("pause")
        player.destroy();media.release();unregisterReceiver(noisy)
        if(wake.isHeld)wake.release();audio.abandonAudioFocusRequest(focus);instance=null
        if(multicast?.isHeld == true) multicast?.release()
        multicastUsers = 0
        super.onDestroy()
    }
}
