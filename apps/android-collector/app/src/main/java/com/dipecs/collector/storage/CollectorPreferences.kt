package com.dipecs.collector.storage

import android.content.Context

object CollectorPreferences {
    const val MODE_MOCK = "mock"
    const val MODE_LLM = "llm"

    private const val PREFS_NAME = "dipecs_collector"
    private const val KEY_UPLOAD_MODE = "upload_mode"
    private const val KEY_ENDPOINT = "endpoint"
    private const val KEY_API_KEY = "api_key"
    private const val KEY_LAST_USAGE_QUERY_MS = "last_usage_query_ms"
    private const val KEY_FOREGROUND_PACKAGE = "foreground_package"
    private const val KEY_FOREGROUND_CLASS = "foreground_class"
    private const val KEY_SOURCE_USAGE = "source_usage_enabled"
    private const val KEY_SOURCE_NOTIFICATION = "source_notification_enabled"
    private const val KEY_SOURCE_ACCESSIBILITY = "source_accessibility_enabled"
    private const val KEY_SOURCE_DEVICE_CONTEXT = "source_device_context_enabled"

    fun uploadMode(context: Context): String =
        prefs(context).getString(KEY_UPLOAD_MODE, MODE_MOCK) ?: MODE_MOCK

    fun setUploadMode(context: Context, mode: String) {
        prefs(context).edit().putString(KEY_UPLOAD_MODE, mode).apply()
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
        prefs(context).getBoolean(KEY_SOURCE_ACCESSIBILITY, true)

    fun setAccessibilityEnabled(context: Context, enabled: Boolean) {
        prefs(context).edit().putBoolean(KEY_SOURCE_ACCESSIBILITY, enabled).apply()
    }

    fun isDeviceContextEnabled(context: Context): Boolean =
        prefs(context).getBoolean(KEY_SOURCE_DEVICE_CONTEXT, true)

    fun setDeviceContextEnabled(context: Context, enabled: Boolean) {
        prefs(context).edit().putBoolean(KEY_SOURCE_DEVICE_CONTEXT, enabled).apply()
    }

    private fun prefs(context: Context) =
        context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
}
