package io.ytmlisten.player

import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.os.Bundle
import com.google.android.gms.auth.api.identity.Identity

/** Google Play services owns account selection and consent, never our player WebView. */
class GoogleAccountActivity : Activity() {
    private var waitingForConsent = false
    private var completed = false
    private var connectionEpoch = -1
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        waitingForConsent = savedInstanceState?.getBoolean("waiting_for_consent") == true
        connectionEpoch = savedInstanceState?.getInt("connection_epoch", -1) ?: -1
        if (!waitingForConsent) {
            connectionEpoch = GoogleLibraryAccount.connect(this,
                resolve = { pending ->
                    waitingForConsent = true
                    startIntentSenderForResult(pending.intentSender, 7342, null, 0, 0, 0)
                },
                complete = ::finishConnection)
        }
    }
    override fun onSaveInstanceState(outState: Bundle) {
        outState.putBoolean("waiting_for_consent", waitingForConsent)
        outState.putInt("connection_epoch", connectionEpoch)
        super.onSaveInstanceState(outState)
    }
    override fun onDestroy() {
        if (isFinishing && !completed && connectionEpoch >= 0) GoogleLibraryAccount.cancel(connectionEpoch)
        super.onDestroy()
    }
    private fun finishConnection(error: String?) {
        if (isFinishing || isDestroyed) return
        if (error == null) { completed = true; finish() } else showFailure(error)
    }

    @Deprecated("Activity result bridge for this dedicated authorization activity")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != 7342) return
        waitingForConsent = false
        if (resultCode != RESULT_OK || data == null) {
            GoogleLibraryAccount.cancel(connectionEpoch)
            finish()
            return
        }
        try {
            GoogleLibraryAccount.accept(this, Identity.getAuthorizationClient(this).getAuthorizationResultFromIntent(data), complete = ::finishConnection, version = connectionEpoch)
        } catch (error: Exception) {
            GoogleLibraryAccount.cancel(connectionEpoch)
            showFailure(GoogleLibraryAccount.explain(this, error))
        }
    }

    private fun showFailure(message: String) {
        AlertDialog.Builder(this).setTitle("Google library connection")
            .setMessage(message)
            .setPositiveButton("Close") { _, _ -> finish() }
            .setNeutralButton("Copy setup details") { _, _ ->
                getSystemService(ClipboardManager::class.java).setPrimaryClip(
                    ClipData.newPlainText("Android OAuth setup", GoogleLibraryAccount.setupDetails(this)))
                finish()
            }.setOnCancelListener { finish() }.show()
    }
}
