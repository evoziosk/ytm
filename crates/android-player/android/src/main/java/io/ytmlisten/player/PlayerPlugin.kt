package io.ytmlisten.player

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.os.Build
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import androidx.core.content.ContextCompat

@InvokeArg class InitializeArgs { lateinit var bridge: String; lateinit var handler: Channel }
@InvokeArg class EvaluateArgs { lateinit var script: String }
@InvokeArg class NavigateArgs { lateinit var url: String }

@TauriPlugin
class PlayerPlugin(private val activity: Activity): Plugin(activity) {
    @Command fun discoveryMulticast(invoke: Invoke) {
        val enabled = invoke.getArgs().getBoolean("enabled")
        activity.runOnUiThread {
            val service = PlayerService.instance
            if (service == null) { invoke.reject("Android playback service is not ready for LAN discovery"); return@runOnUiThread }
            try { service.discoveryMulticast(enabled); invoke.resolve() }
            catch (e: Exception) { invoke.reject("Could not enable LAN discovery: ${e.message}") }
        }
    }
    @Command fun remoteState(invoke: Invoke) {
        val status = invoke.getArgs().getJSONObject("status")
        activity.runOnUiThread {
            PlayerService.instance?.remoteState(status)
            invoke.resolve()
        }
    }
    @Command fun initialize(invoke: Invoke) {
        val args = invoke.parseArgs(InitializeArgs::class.java)
        activity.runOnUiThread {
            PlayerService.bridge = args.bridge
            PlayerService.channel = args.handler
            if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(activity, Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 7341)
            }
            ContextCompat.startForegroundService(activity, Intent(activity, PlayerService::class.java))
            GoogleLibraryAccount.initialize(activity, args.handler)
            invoke.resolve()
        }
    }
    @Command fun evaluate(invoke: Invoke) {
        val args = invoke.parseArgs(EvaluateArgs::class.java)
        activity.runOnUiThread {
            val service = PlayerService.instance
            if (service == null || service.player.url?.startsWith("https://music.youtube.com/") != true) invoke.reject("Return to YouTube Music after signing in")
            else { service.player.evaluateJavascript(args.script, null); invoke.resolve() }
        }
    }
    @Command fun navigate(invoke: Invoke) {
        val args = invoke.parseArgs(NavigateArgs::class.java)
        activity.runOnUiThread {
            if (!PlayerService.allowed(args.url)) { invoke.reject("Invalid player destination"); return@runOnUiThread }
            PlayerService.instance?.player?.loadUrl(args.url)
            invoke.resolve()
        }
    }
    @Command fun account(invoke: Invoke) {
        activity.runOnUiThread {
            activity.startActivity(Intent(activity, GoogleAccountActivity::class.java))
            invoke.resolve()
        }
    }
    @Command fun disconnectAccount(invoke: Invoke) {
        activity.runOnUiThread {
            GoogleLibraryAccount.disconnect(activity) { error ->
                if (error == null) invoke.resolve() else invoke.reject(error)
            }
        }
    }
    @Command fun libraryCatalog(invoke: Invoke) {
        val request = invoke.getArgs().getJSONObject("request")
        activity.runOnUiThread {
            GoogleLibraryAccount.library(activity, request) { page, error ->
                if (page != null) invoke.resolve(app.tauri.plugin.JSObject(page.toString()))
                else invoke.reject(error ?: "Could not load your Google library")
            }
        }
    }
}
