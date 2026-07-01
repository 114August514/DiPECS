package com.dipecs.collector.model

import org.json.JSONArray
import org.json.JSONObject

object AndroidRawEventMapper {

    // Rust uses serde's externally tagged enum representation for RawEvent.
    // Each mapper here therefore returns {"VariantName": {...payload...}}.

    fun appTransition(
        timestampMs: Long,
        packageName: String,
        activityClass: String?,
        transition: String,
    ): JSONObject = tagged(
        "AppTransition",
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("package_name", packageName)
            .put("activity_class", activityClass ?: JSONObject.NULL)
            .put("transition", transition),
    )

    fun notificationPosted(
        timestampMs: Long,
        packageName: String,
        category: String?,
        channelId: String?,
        title: String?,
        textItems: List<String>,
        isOngoing: Boolean,
        hasPicture: Boolean,
    ): JSONObject = tagged(
        "NotificationPosted",
        JSONObject()
            // Transport metadata: safe to preserve because downstream context
            // and policy checks need package/channel-level routing signals.
            .put("timestamp_ms", timestampMs)
            .put("package_name", packageName)
            .put("category", category ?: JSONObject.NULL)
            .put("channel_id", channelId ?: JSONObject.NULL)

            // Raw notification text is intentionally never persisted by the
            // Android app. The safe hints below are the only text-derived data
            // that crosses into the Rust pipeline.
            .put("raw_title", "")
            .put("raw_text", "")

            // Privacy-preserving features. These match the Rust-side
            // TextHint/SemanticHint enum names exactly so serde can ingest
            // them without schema translation.
            .put("title_hint", textHint(title.orEmpty()))
            .put("text_hint", textHint(textItems.joinToString(separator = " ")))
            .put("semantic_hints", semanticHints(title.orEmpty(), textItems, hasPicture))

            // The Android notification key/group tag can contain conversation
            // or contact identifiers, so it stays null even for local traces.
            .put("is_ongoing", isOngoing)
            .put("group_key", JSONObject.NULL)
            .put("has_picture", hasPicture),
    )

    fun notificationInteraction(
        timestampMs: Long,
        packageName: String,
        action: String,
    ): JSONObject = tagged(
        "NotificationInteraction",
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("package_name", packageName)
            .put("notification_key", "")
            .put("action", action),
    )

    fun screenState(timestampMs: Long, state: String): JSONObject = tagged(
        "ScreenState",
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("state", state),
    )

    fun systemState(timestampMs: Long, context: DeviceContext): JSONObject = tagged(
        "SystemState",
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("battery_pct", context.batteryPercent ?: JSONObject.NULL)
            .put("is_charging", context.isCharging ?: false)
            .put("network", rustNetwork(context.networkType))
            .put("ringer_mode", rustRingerMode(context.ringerMode))
            .put("location_type", rustLocationType(context.locationType))
            .put("headphone_connected", context.headphoneConnected)
            .put("bluetooth_connected", context.bluetoothConnected),
    )

    fun rawEventKind(rawEvent: JSONObject?): String? {
        rawEvent ?: return null
        val keys = rawEvent.keys()
        return if (keys.hasNext()) keys.next() else null
    }

    private fun tagged(kind: String, payload: JSONObject): JSONObject =
        JSONObject().put(kind, payload)

    // ===== Rust enum value adapters =====

    private fun rustNetwork(networkType: String): String = when (networkType) {
        "wifi", "ethernet", "bluetooth", "vpn" -> "Wifi"
        "cellular" -> "Cellular"
        "offline" -> "Offline"
        else -> "Unknown"
    }

    private fun rustRingerMode(ringerMode: String): String = when (ringerMode) {
        "normal" -> "Normal"
        "vibrate" -> "Vibrate"
        "silent" -> "Silent"
        else -> "Normal"
    }

    private fun rustLocationType(locationType: String): String = when (locationType.lowercase()) {
        "home" -> "Home"
        "work" -> "Work"
        "commute" -> "Commute"
        else -> "Unknown"
    }

    // ===== Privacy-preserving notification feature extraction =====

    private fun textHint(text: String): JSONObject = JSONObject()
        .put("length_chars", text.codePointCount(0, text.length))
        .put("script", scriptHint(text))
        .put(
            "is_emoji_only",
            text.isNotEmpty() && text.codePoints().allMatch { isEmojiCodePoint(it) },
        )

    private fun semanticHints(title: String, textItems: List<String>, hasPicture: Boolean): JSONArray {
        val combined = (listOf(title) + textItems).joinToString(separator = " ").lowercase()
        val hints = linkedSetOf<String>()

        // Keep these keyword groups aligned with crates/aios-core/src/text_analysis.rs.
        // Android computes the same categories locally, then drops the original text.

        if (containsAny(
                combined,
                listOf(
                    "文件", "file", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
                    "zip", "rar", "attachment", "附件",
                ),
            )
        ) {
            hints += "FileMention"
        }
        if (hasPicture || containsAny(
                combined,
                listOf(
                    "图片", "照片", "截图", "image", "photo", "screenshot", "jpg", "jpeg",
                    "png", "gif", "webp", "相册",
                ),
            )
        ) {
            hints += "ImageMention"
        }
        if (containsAny(
                combined,
                listOf("语音", "voice", "audio", "mp3", "wav", "aac", "录音", "通话"),
            )
        ) {
            hints += "AudioMessage"
        }
        if (containsAny(combined, listOf("http", "https", "www.", "链接", "link", "url"))) {
            hints += "LinkAttachment"
        }
        if (containsAny(combined, listOf("@你", "@所有人", "提到了你", "mentioned you", "@"))) {
            hints += "UserMentioned"
        }
        if (containsAny(
                combined,
                listOf(
                    "会议", "meeting", "calendar", "日历", "invitation", "邀请", "schedule", "日程",
                ),
            )
        ) {
            hints += "CalendarInvitation"
        }
        if (containsAny(
                combined,
                listOf("支付", "付款", "转账", "payment", "transaction", "红包", "balance", "余额"),
            )
        ) {
            hints += "FinancialContext"
        }
        if (containsAny(
                combined,
                listOf("验证码", "code", "otp", "验证", "verification", "captcha"),
            )
        ) {
            hints += "VerificationCode"
        }

        return JSONArray().also { array -> hints.forEach(array::put) }
    }

    // This lightweight script detector mirrors the Rust sanitizer. It is not a
    // language classifier; it only preserves broad writing-system metadata.

    private fun scriptHint(text: String): String {
        if (text.isEmpty()) {
            return "Unknown"
        }

        var hasLatin = false
        var hasHanzi = false
        var hasCyrillic = false
        var hasArabic = false
        text.forEach { ch ->
            when (ch) {
                in '\u0041'..'\u007A', in '\u00C0'..'\u024F' -> hasLatin = true
                in '\u4E00'..'\u9FFF',
                in '\u3400'..'\u4DBF',
                in '\u3000'..'\u303F',
                in '\uFF00'..'\uFFEF' -> hasHanzi = true
                in '\u0400'..'\u04FF', in '\u0500'..'\u052F' -> hasCyrillic = true
                in '\u0600'..'\u06FF',
                in '\u0750'..'\u077F',
                in '\uFB50'..'\uFDFF',
                in '\uFE70'..'\uFEFF' -> hasArabic = true
            }
        }

        val count = listOf(hasLatin, hasHanzi, hasCyrillic, hasArabic).count { it }
        return when {
            count == 0 -> "Unknown"
            count == 1 && hasLatin -> "Latin"
            count == 1 && hasHanzi -> "Hanzi"
            count == 1 && hasCyrillic -> "Cyrillic"
            count == 1 && hasArabic -> "Arabic"
            else -> "Mixed"
        }
    }

    private fun containsAny(text: String, keywords: List<String>): Boolean =
        keywords.any { text.contains(it) }

    // Kotlin Char iterates UTF-16 code units, so emoji detection uses code
    // points to keep surrogate pairs from being split and misclassified.

    private fun isEmojiCodePoint(codePoint: Int): Boolean =
        codePoint in 0x1F600..0x1F64F ||
            codePoint in 0x1F300..0x1F5FF ||
            codePoint in 0x1F680..0x1F6FF ||
            codePoint in 0x1F900..0x1F9FF ||
            codePoint in 0x2600..0x26FF ||
            codePoint in 0x2700..0x27BF ||
            codePoint in 0xFE00..0xFE0F ||
            codePoint == 0x200D ||
            codePoint in 0x1F1E0..0x1F1FF
}
