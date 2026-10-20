package io.ytmlisten.player

import android.accounts.Account
import android.app.Activity
import android.app.PendingIntent
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject
import com.google.android.gms.auth.api.identity.AuthorizationRequest
import com.google.android.gms.auth.api.identity.AuthorizationResult
import com.google.android.gms.auth.api.identity.ClearTokenRequest
import com.google.android.gms.auth.api.identity.Identity
import com.google.android.gms.auth.api.identity.RevokeAccessRequest
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
import com.google.android.gms.common.api.ApiException
import com.google.android.gms.common.api.Scope
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder
import java.security.MessageDigest
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

/** Google owns token caching. Bearer tokens never cross the Tauri/page bridge or enter preferences. */
object GoogleLibraryAccount {
    private const val READ_ONLY = "https://www.googleapis.com/auth/youtube.readonly"
    private val scopes = listOf(READ_ONLY, "https://www.googleapis.com/auth/userinfo.email", "https://www.googleapis.com/auth/userinfo.profile")
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newFixedThreadPool(2)
    private val epoch = AtomicInteger()
    private val connecting = AtomicBoolean()
    @Volatile private var pendingEpoch = 0
    @Volatile private var channel: Channel? = null
    private fun preferences(context: Context) = context.getSharedPreferences("google_library", Context.MODE_PRIVATE)

    fun initialize(activity: Activity, handler: Channel) {
        channel = handler
        if (preferences(activity).getBoolean("connected", false)) {
            authorize(activity, interactive = false, resolve = {}, complete = {})
        } else publish(false, "", "Connect Google to browse your YouTube playlists. Playback has a separate session.")
    }

    fun connect(activity: Activity, resolve: (PendingIntent) -> Unit, complete: (String?) -> Unit): Int {
        if (!connecting.compareAndSet(false, true)) {
            complete("Google authorization is already open.")
            return -1
        }
        pendingEpoch = epoch.incrementAndGet()
        authorize(activity, interactive = true, resolve = resolve, complete = complete)
        return pendingEpoch
    }

    private fun authorize(activity: Activity, interactive: Boolean, resolve: (PendingIntent) -> Unit, complete: (String?) -> Unit) {
        val version = epoch.get()
        if (GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(activity) != ConnectionResult.SUCCESS) {
            connecting.set(false)
            val message = "Google Play services must be installed and up to date to connect your library."
            publish(false, "", message)
            complete(message)
            return
        }
        val request = request(activity)
        Identity.getAuthorizationClient(activity).authorize(request)
            .addOnSuccessListener { result ->
                if (epoch.get() != version) return@addOnSuccessListener
                if (result.hasResolution()) {
                    if (interactive) {
                        try { resolve(requireNotNull(result.pendingIntent)) }
                        catch (error: Exception) { connecting.set(false); complete(explain(activity, error)) }
                    } else {
                        publish(false, "", "Reconnect Google to restore access to your playlists.")
                        complete("Reconnect Google to restore access to your playlists.")
                    }
                } else accept(activity, result, complete, version)
            }.addOnFailureListener { error ->
                if (epoch.get() == version) {
                    connecting.set(false)
                    val message = explain(activity, error)
                    publish(false, "", message)
                    complete(message)
                }
            }
    }

    private fun request(context: Context): AuthorizationRequest {
        val builder = AuthorizationRequest.builder().setRequestedScopes(scopes.map { Scope(it) })
        preferences(context).getString("email", null)?.takeIf { it.isNotBlank() }?.let {
            builder.setAccount(Account(it, "com.google"))
        }
        return builder.build()
    }

    fun accept(activity: Activity, result: AuthorizationResult, complete: (String?) -> Unit, version: Int = pendingEpoch) {
        if (epoch.get() != version) { complete("Authorization was cancelled."); return }
        val token = result.accessToken
        if (token.isNullOrBlank() || !result.grantedScopes.containsAll(scopes)) {
            connecting.set(false)
            complete("Grant YouTube library and profile access to connect your account.")
            return
        }
        worker.execute {
            try {
                val profile = get("oauth2/v3/userinfo", emptyMap(), token)
                val email = profile.optString("email")
                require(email.isNotBlank() && profile.optBoolean("email_verified")) { "Google did not return a verified account." }
                val name = profile.optString("name").ifBlank { "Google library" }.take(512)
                main.post {
                    if (epoch.get() != version) { complete("Authorization was cancelled."); return@post }
                    preferences(activity).edit().putBoolean("connected", true).putString("email", email).putString("name", name).apply()
                    connecting.set(false)
                    publish(true, name, "Google library connected. Playback uses a separate session; Music Premium sign-in is not enabled.")
                    complete(null)
                }
            } catch (error: Exception) {
                main.post {
                    if (epoch.get() == version) {
                        connecting.set(false)
                        val message = explain(activity, error)
                        publish(false, "", message)
                        complete(message)
                    }
                }
            }
        }
    }

    fun cancel(expectedEpoch: Int? = null) {
        if (expectedEpoch != null && epoch.get() != expectedEpoch) return
        connecting.set(false)
        epoch.incrementAndGet()
    }

    fun disconnect(activity: Activity, complete: (String?) -> Unit) {
        cancel()
        val email = preferences(activity).getString("email", null)
        preferences(activity).edit().clear().apply()
        publish(false, "", "Google library disconnected.")
        if (email == null) { complete(null); return }
        val request = RevokeAccessRequest.builder().setAccount(Account(email, "com.google"))
            .setScopes(scopes.map { Scope(it) }).build()
        Identity.getAuthorizationClient(activity).revokeAccess(request)
            .addOnSuccessListener { complete(null) }
            .addOnFailureListener {
                val message = "Disconnected on this phone. Google could not revoke access; remove YTM Listen in your Google Account's third-party connections."
                publish(false, "", message)
                complete(message)
            }
    }

    fun library(activity: Activity, catalogRequest: JSONObject, complete: (JSONObject?, String?) -> Unit) {
        if (!preferences(activity).getBoolean("connected", false)) {
            complete(null, "Connect Google in Account settings to browse your YouTube playlists.")
            return
        }
        val version = epoch.get()
        // Ask Google for a current cached token; never launch consent from a catalog query.
        Identity.getAuthorizationClient(activity).authorize(request(activity))
            .addOnSuccessListener { result ->
                if (version != epoch.get()) { complete(null, "Account changed; retry your library."); return@addOnSuccessListener }
                val token = result.accessToken
                if (result.hasResolution() || token.isNullOrBlank() || !result.grantedScopes.contains(READ_ONLY)) {
                    publish(false, "", "Reconnect Google to restore access to your playlists.")
                    complete(null, "Reconnect Google in Account settings.")
                    return@addOnSuccessListener
                }
                worker.execute {
                    try {
                        val page = YouTubeLibrary.query(catalogRequest) { endpoint, params -> get("youtube/v3/$endpoint", params, token) }
                        if (version != epoch.get()) complete(null, "Account changed; retry your library.")
                        else complete(page, null)
                    } catch (error: Exception) {
                        if (error is GoogleHttpError && error.status == 401) {
                            main.post {
                                Identity.getAuthorizationClient(activity).clearToken(ClearTokenRequest.builder().setToken(token).build())
                                if (version == epoch.get()) publish(false, "", "Google authorization expired. Reconnect your library.")
                            }
                        }
                        complete(null, explain(activity, error))
                    }
                }
            }.addOnFailureListener { complete(null, explain(activity, it)) }
    }

    private class GoogleHttpError(val status: Int) : Exception("Google API request failed ($status)")
    private fun get(path: String, params: Map<String, String>, token: String): JSONObject {
        require(path in setOf("oauth2/v3/userinfo", "youtube/v3/playlists", "youtube/v3/playlistItems"))
        val query = params.entries.joinToString("&") { "${URLEncoder.encode(it.key, "UTF-8")}=${URLEncoder.encode(it.value, "UTF-8")}" }
        val connection = URL("https://www.googleapis.com/$path${if (query.isEmpty()) "" else "?$query"}").openConnection() as HttpURLConnection
        try {
            connection.connectTimeout = 10000
            connection.readTimeout = 15000
            connection.instanceFollowRedirects = false
            connection.setRequestProperty("Authorization", "Bearer $token")
            connection.setRequestProperty("Accept", "application/json")
            if (connection.responseCode != 200) throw GoogleHttpError(connection.responseCode)
            val body = StringBuilder()
            connection.inputStream.bufferedReader(Charsets.UTF_8).use { reader ->
                val buffer = CharArray(8192)
                while (true) {
                    val count = reader.read(buffer)
                    if (count < 0) break
                    require(body.length + count <= 2_000_000) { "Google response was too large." }
                    body.append(buffer, 0, count)
                }
            }
            return JSONObject(body.toString())
        } finally { connection.disconnect() }
    }

    private fun publish(signedIn: Boolean, name: String, message: String) {
        channel?.send(JSObject().put("kind", "google_library_account").put("payload",
            JSObject().put("signed_in", signedIn).put("name", name.take(512)).put("message", message.take(2048))))
    }

    fun explain(context: Context, error: Exception): String = when {
        error is ApiException && error.statusCode == 10 -> "This APK is not registered for Google authorization. Enable YouTube Data API v3 and create an Android OAuth client with these exact details:\n\n${setupDetails(context)}"
        error is ApiException && error.statusCode == 16 -> "Google connection cancelled."
        error is GoogleHttpError && error.status == 403 -> "Google denied library access. Enable YouTube Data API v3, check the consent-screen test users and granted permissions, and verify the API quota."
        error is GoogleHttpError && error.status == 401 -> "Google authorization expired. Reconnect Google in Account settings."
        error is ApiException -> "Google authorization failed (code ${error.statusCode}). Check Play services and the OAuth setup, then retry."
        error is java.io.IOException -> "Could not reach Google. Check your connection and retry."
        else -> "Could not connect your Google library. Check the OAuth setup and retry."
    }

    @Suppress("DEPRECATION")
    fun setupDetails(context: Context): String {
        val info = context.packageManager.getPackageInfo(context.packageName,
            if (Build.VERSION.SDK_INT >= 28) PackageManager.GET_SIGNING_CERTIFICATES else PackageManager.GET_SIGNATURES)
        val signatures = if (Build.VERSION.SDK_INT >= 28) info.signingInfo?.apkContentsSigners else info.signatures
        val fingerprints = signatures.orEmpty().map { signature ->
            MessageDigest.getInstance("SHA-1").digest(signature.toByteArray()).joinToString(":") { "%02X".format(it.toInt() and 255) }
        }
        return "Package: ${context.packageName}\nSHA-1: ${fingerprints.joinToString(", ")}\nScopes: ${scopes.joinToString(" ")}"
    }
}
