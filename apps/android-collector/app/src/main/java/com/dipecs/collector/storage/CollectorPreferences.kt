package com.dipecs.collector.storage

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import com.dipecs.collector.BuildConfig
import java.security.SecureRandom

object CollectorPreferences {
    const val MODE_MOCK = "mock"
    const val MODE_LLM = "llm"
    const val DEFAULT_ACTION_SOCKET_PORT = 46321
    const val DEBUG_ACTION_SOCKET_TOKEN = "dipecs-dev-emulator-shared-token-00000000"

    private const val LEGACY_PREFS_NAME = "dipecs_collector"
    private const val SECURE_PREFS_NAME = "dipecs_collector_secure"
    private const val KEY_UPLOAD_MODE = "upload_mode"
    private const val KEY_UPLOAD_ENABLED = "upload_enabled"
    private const val KEY_ENDPOINT = "endpoint"
    private const val KEY_API_KEY = "api_key"
    private const val KEY_PREFETCH_TARGET = "prefetch_target"
    private const val KEY_AUTHORIZED_ACTION_JSON = "authorized_action_json"
    private const val KEY_ACTION_SOCKET_PORT = "action_socket_port"
    private const val KEY_ACTION_SOCKET_TOKEN = "action_socket_token"
    private const val KEY_LAST_USAGE_QUERY_MS = "last_usage_query_ms"
    private const val KEY_FOREGROUND_PACKAGE = "foreground_package"
    private const val KEY_FOREGROUND_CLASS = "foreground_class"
    private const val KEY_SOURCE_USAGE = "source_usage_enabled"
    private const val KEY_SOURCE_NOTIFICATION = "source_notification_enabled"
    private const val KEY_SOURCE_ACCESSIBILITY = "source_accessibility_enabled"
    private const val KEY_SOURCE_DEVICE_CONTEXT = "source_device_context_enabled"
    private const val KEY_COLLECTOR_RUNNING = "collector_running"
    private const val KEY_COLLECTOR_LAST_STARTED_MS = "collector_last_started_ms"
    private const val KEY_COLLECTOR_LAST_STOPPED_MS = "collector_last_stopped_ms"
    private const val KEY_LAST_HEARTBEAT_MS = "last_heartbeat_ms"
    private const val KEY_ACTION_SOCKET_LISTENING = "action_socket_listening"
    private const val KEY_ACTION_SOCKET_STATUS = "action_socket_status"
    private const val KEY_ACTION_SOCKET_STATUS_MS = "action_socket_status_ms"
    private const val KEY_LAST_EXPORT_PATH = "last_export_path"
    private const val KEY_LAST_EXPORT_MS = "last_export_ms"
    private const val DEBUG_TOKEN_PROPERTY = "debug.dipecs.token"

    @Volatile
    private var legacyMigrationDone = false

    fun uploadMode(context: Context): String =
        prefs(context).getString(KEY_UPLOAD_MODE, MODE_MOCK) ?: MODE_MOCK

    fun setUploadMode(context: Context, mode: String) {
        prefs(context).edit().putString(KEY_UPLOAD_MODE, mode).apply()
    }

    fun isUploadEnabled(context: Context): Boolean =
        prefs(context).getBoolean(KEY_UPLOAD_ENABLED, false)

    fun setUploadEnabled(context: Context, enabled: Boolean) {
        prefs(context).edit().putBoolean(KEY_UPLOAD_ENABLED, enabled).apply()
    }

    fun endpoint(context: Context): String =
        prefs(context).getString(KEY_ENDPOINT, "") ?: ""

    fun setEndpoint(context: Context, endpoint: String) {
        prefs(context).edit().putString(KEY_ENDPOINT, endpoint.trim()).apply()
    }

    fun apiKey(context: Context): String =
        prefs(context).getString(KEY_API_KEY, "") ?: ""

    fun setApiKey(context: Context, apiKey: String) {
        prefs(context).edit().putString(KEY_API_KEY, apiKey.trim()).apply()
    }

    fun prefetchTarget(context: Context): String =
        prefs(context).getString(KEY_PREFETCH_TARGET, "") ?: ""

    fun setPrefetchTarget(context: Context, target: String) {
        prefs(context).edit().putString(KEY_PREFETCH_TARGET, target.trim()).apply()
    }

    fun authorizedActionJson(context: Context): String =
        prefs(context).getString(KEY_AUTHORIZED_ACTION_JSON, "") ?: ""

    fun setAuthorizedActionJson(context: Context, payload: String) {
        prefs(context).edit().putString(KEY_AUTHORIZED_ACTION_JSON, payload.trim()).apply()
    }

    fun actionSocketPort(context: Context): Int =
        prefs(context).getInt(KEY_ACTION_SOCKET_PORT, DEFAULT_ACTION_SOCKET_PORT)

    fun setActionSocketPort(context: Context, port: Int) {
        prefs(context).edit().putInt(KEY_ACTION_SOCKET_PORT, port).apply()
    }

    fun actionSocketToken(context: Context): String {
        synchronized(this) {
            val securePrefs = prefs(context)
            val existing = securePrefs.getString(KEY_ACTION_SOCKET_TOKEN, null)
                ?.takeIf { it.isNotBlank() }
            if (existing != null) {
                return existing
            }

            val generated = initialActionSocketToken()
            val saved = securePrefs.edit()
                .putString(KEY_ACTION_SOCKET_TOKEN, generated)
                .commit()
            check(saved) { "Failed to persist action socket token" }
            return generated
        }
    }

    fun lastUsageQueryMs(context: Context): Long =
        prefs(context).getLong(KEY_LAST_USAGE_QUERY_MS, System.currentTimeMillis() - 60_000L)

    fun setLastUsageQueryMs(context: Context, value: Long) {
        prefs(context).edit().putLong(KEY_LAST_USAGE_QUERY_MS, value).apply()
    }

    fun setForeground(context: Context, packageName: String?, className: String?) {
        prefs(context).edit()
            .putString(KEY_FOREGROUND_PACKAGE, packageName)
            .putString(KEY_FOREGROUND_CLASS, className)
            .apply()
    }

    fun foregroundPackage(context: Context): String? =
        prefs(context).getString(KEY_FOREGROUND_PACKAGE, null)

    fun foregroundClass(context: Context): String? =
        prefs(context).getString(KEY_FOREGROUND_CLASS, null)

    fun isUsageEnabled(context: Context): Boolean =
        prefs(context).getBoolean(KEY_SOURCE_USAGE, true)

    fun setUsageEnabled(context: Context, enabled: Boolean) {
        prefs(context).edit().putBoolean(KEY_SOURCE_USAGE, enabled).apply()
    }

    fun isNotificationEnabled(context: Context): Boolean =
        prefs(context).getBoolean(KEY_SOURCE_NOTIFICATION, true)

    fun setNotificationEnabled(context: Context, enabled: Boolean) {
        prefs(context).edit().putBoolean(KEY_SOURCE_NOTIFICATION, enabled).apply()
    }

    fun isAccessibilityEnabled(context: Context): Boolean =
        prefs(context).getBoolean(KEY_SOURCE_ACCESSIBILITY, false)

    fun setAccessibilityEnabled(context: Context, enabled: Boolean) {
        prefs(context).edit().putBoolean(KEY_SOURCE_ACCESSIBILITY, enabled).apply()
    }

    fun isDeviceContextEnabled(context: Context): Boolean =
        prefs(context).getBoolean(KEY_SOURCE_DEVICE_CONTEXT, true)

    fun setDeviceContextEnabled(context: Context, enabled: Boolean) {
        prefs(context).edit().putBoolean(KEY_SOURCE_DEVICE_CONTEXT, enabled).apply()
    }

    fun isCollectorRunning(context: Context): Boolean =
        prefs(context).getBoolean(KEY_COLLECTOR_RUNNING, false)

    fun setCollectorRunning(context: Context, running: Boolean) {
        val now = System.currentTimeMillis()
        val editor = prefs(context).edit()
            .putBoolean(KEY_COLLECTOR_RUNNING, running)
        if (running) {
            editor.putLong(KEY_COLLECTOR_LAST_STARTED_MS, now)
        } else {
            editor.putLong(KEY_COLLECTOR_LAST_STOPPED_MS, now)
        }
        editor.apply()
    }

    fun collectorLastStartedMs(context: Context): Long =
        prefs(context).getLong(KEY_COLLECTOR_LAST_STARTED_MS, 0L)

    fun collectorLastStoppedMs(context: Context): Long =
        prefs(context).getLong(KEY_COLLECTOR_LAST_STOPPED_MS, 0L)

    fun setLastHeartbeatMs(context: Context, timestampMs: Long) {
        prefs(context).edit().putLong(KEY_LAST_HEARTBEAT_MS, timestampMs).apply()
    }

    fun lastHeartbeatMs(context: Context): Long =
        prefs(context).getLong(KEY_LAST_HEARTBEAT_MS, 0L)

    fun isActionSocketListening(context: Context): Boolean =
        prefs(context).getBoolean(KEY_ACTION_SOCKET_LISTENING, false)

    fun setActionSocketStatus(context: Context, listening: Boolean, status: String) {
        prefs(context).edit()
            .putBoolean(KEY_ACTION_SOCKET_LISTENING, listening)
            .putString(KEY_ACTION_SOCKET_STATUS, status)
            .putLong(KEY_ACTION_SOCKET_STATUS_MS, System.currentTimeMillis())
            .apply()
    }

    fun actionSocketStatus(context: Context): String =
        prefs(context).getString(KEY_ACTION_SOCKET_STATUS, "not started") ?: "not started"

    fun actionSocketStatusMs(context: Context): Long =
        prefs(context).getLong(KEY_ACTION_SOCKET_STATUS_MS, 0L)

    fun setLastExport(context: Context, path: String, timestampMs: Long) {
        prefs(context).edit()
            .putString(KEY_LAST_EXPORT_PATH, path)
            .putLong(KEY_LAST_EXPORT_MS, timestampMs)
            .apply()
    }

    fun lastExportPath(context: Context): String =
        prefs(context).getString(KEY_LAST_EXPORT_PATH, "") ?: ""

    fun lastExportMs(context: Context): Long =
        prefs(context).getLong(KEY_LAST_EXPORT_MS, 0L)

    private fun prefs(context: Context): SharedPreferences {
        val appContext = context.applicationContext
        val securePrefs = encryptedPrefs(appContext)
        migrateLegacyPrefs(appContext, securePrefs)
        return securePrefs
    }

    private fun encryptedPrefs(context: Context): SharedPreferences {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        return EncryptedSharedPreferences.create(
            context,
            SECURE_PREFS_NAME,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
    }

    private fun migrateLegacyPrefs(context: Context, securePrefs: SharedPreferences) {
        if (legacyMigrationDone) {
            return
        }

        val legacyPrefs = context.getSharedPreferences(LEGACY_PREFS_NAME, Context.MODE_PRIVATE)
        synchronized(this) {
            if (legacyMigrationDone) {
                return
            }
            if (legacyPrefs.all.isNotEmpty()) {
                val editor = securePrefs.edit()
                legacyPrefs.all.forEach { (key, value) ->
                    when (value) {
                        is String -> editor.putString(key, value)
                        is Int -> editor.putInt(key, value)
                        is Long -> editor.putLong(key, value)
                        is Boolean -> editor.putBoolean(key, value)
                        is Float -> editor.putFloat(key, value)
                        is Set<*> -> editor.putStringSet(key, value.filterIsInstance<String>().toSet())
                    }
                }
                editor.commit()
                legacyPrefs.edit().clear().commit()
            }
            legacyMigrationDone = true
        }
    }

    private fun initialActionSocketToken(): String {
        if (BuildConfig.DEBUG) {
            return debugInjectedToken() ?: DEBUG_ACTION_SOCKET_TOKEN
        }
        return generateSocketToken()
    }

    private fun debugInjectedToken(): String? {
        if (!BuildConfig.DEBUG) {
            return null
        }
        return runCatching {
            val clazz = Class.forName("android.os.SystemProperties")
            val getter = clazz.getMethod("get", String::class.java)
            (getter.invoke(null, DEBUG_TOKEN_PROPERTY) as? String)
                ?.trim()
                ?.takeIf { it.isNotBlank() }
        }.getOrNull()
    }

    private fun generateSocketToken(): String {
        val bytes = ByteArray(32)
        SecureRandom().nextBytes(bytes)
        return bytes.joinToString(separator = "") { byte -> "%02x".format(byte) }
    }

}
