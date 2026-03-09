package cloud.opensecret.maple

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import cloud.opensecret.maple.rust.AppAction
import cloud.opensecret.maple.rust.AppReconciler
import cloud.opensecret.maple.rust.AppState
import cloud.opensecret.maple.rust.AppUpdate
import cloud.opensecret.maple.rust.AuthState
import cloud.opensecret.maple.rust.FfiApp
import cloud.opensecret.maple.rust.Router
import cloud.opensecret.maple.rust.Screen

class AppManager private constructor(context: Context) : AppReconciler {
    private val mainHandler = Handler(Looper.getMainLooper())
    private val rust: FfiApp
    private var lastRevApplied: ULong = 0UL

    private val securePrefs = EncryptedSharedPreferences.create(
        context.applicationContext,
        "maple_secure_prefs",
        MasterKey.Builder(context.applicationContext)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build(),
        EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
        EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
    )

    var state: AppState by mutableStateOf(
        AppState(
            rev = 0UL,
            auth = AuthState.Initializing,
            pendingAuthUrl = null,
            router = Router(
                defaultScreen = Screen.LOADING,
                screenStack = emptyList(),
            ),
            messages = emptyList(),
            isAgentTyping = false,
            isLoadingHistory = false,
            hasOlderMessages = false,
            composeText = "",
            toast = null,
            showSettings = false,
            confirmDeleteAgent = false,
            isDeletingAgent = false,
        ),
    )
        private set

    private fun configuredApiUrl(): String {
        val configured = BuildConfig.OPEN_SECRET_API_URL.trim()
        val apiUrl = if (configured.isNotEmpty()) configured else "http://0.0.0.0:3000"

        return if (apiUrl == "http://0.0.0.0:3000") "http://10.0.2.2:3000" else apiUrl
    }

    init {
        val dataDir = context.filesDir.absolutePath

        val apiUrl = configuredApiUrl()
        val clientId = "ba5a14b5-d915-47b1-b7b1-afda52bc5fc6"

        rust = FfiApp(apiUrl = apiUrl, clientId = clientId, dataDir = dataDir)
        val initial = rust.state()
        state = initial
        lastRevApplied = initial.rev
        rust.listenForUpdates(this)

        // Attempt session restore from EncryptedSharedPreferences
        val access = securePrefs.getString("access_token", null)
        val refresh = securePrefs.getString("refresh_token", null)
        if (access != null && refresh != null) {
            rust.dispatch(AppAction.RestoreSession(accessToken = access, refreshToken = refresh))
        }
    }

    fun dispatch(action: AppAction) {
        rust.dispatch(action)
    }

    override fun reconcile(update: AppUpdate) {
        mainHandler.post {
            when (update) {
                is AppUpdate.SessionTokens -> {
                    // Persist side-effect BEFORE rev guard (per bible 6.6)
                    if (update.accessToken.isEmpty()) {
                        securePrefs.edit().remove("access_token").remove("refresh_token").apply()
                    } else {
                        securePrefs.edit()
                            .putString("access_token", update.accessToken)
                            .putString("refresh_token", update.refreshToken)
                            .apply()
                    }
                    if (update.rev > lastRevApplied) {
                        lastRevApplied = update.rev
                    }
                }
                is AppUpdate.FullState -> {
                    if (update.v1.rev <= lastRevApplied) return@post
                    lastRevApplied = update.v1.rev
                    state = update.v1
                }
            }
        }
    }

    companion object {
        @Volatile
        private var instance: AppManager? = null

        fun getInstance(context: Context): AppManager =
            instance ?: synchronized(this) {
                instance ?: AppManager(context.applicationContext).also { instance = it }
            }
    }
}
