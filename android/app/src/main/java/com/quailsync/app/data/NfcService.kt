package com.quailsync.app.data

import android.app.Activity
import android.app.PendingIntent
import android.content.Intent
import android.content.IntentFilter
import android.nfc.NdefMessage
import android.nfc.NdefRecord
import android.nfc.NfcAdapter
import android.nfc.Tag
import android.nfc.tech.MifareUltralight
import android.nfc.tech.Ndef
import android.nfc.tech.NdefFormatable
import android.os.Build
import android.os.Parcelable
import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.io.IOException
import java.time.LocalDateTime

data class NfcScanResult(
    val tagId: String,
    val payload: String?,
    val timestamp: LocalDateTime = LocalDateTime.now(),
    val bird: Bird? = null,
)

/**
 * Returned when a write-mode scan finds existing QuailSync data on the tag.
 * The caller must show a confirmation dialog before proceeding.
 */
data class TagConflict(
    val tagId: String,
    val existingPayload: String,
    val existingBirdId: Int,
    val pendingWriteData: String,
)

/** NTAG21x command opcodes — see NXP NTAG213/215/216 datasheet §10. */
private const val NTAG_CMD_PWD_AUTH: Byte = 0x1B
private const val NTAG_CMD_GET_VERSION: Byte = 0x60

/** Common factory-default NTAG passwords, tried in order. The 0xFFFFFFFF
 *  variant is the canonical NTAG factory state; some preprogrammed batches
 *  use all-zeros instead. */
private val DEFAULT_NTAG_PASSWORDS = listOf(
    byteArrayOf(0xFF.toByte(), 0xFF.toByte(), 0xFF.toByte(), 0xFF.toByte()),
    byteArrayOf(0x00, 0x00, 0x00, 0x00),
)

class NfcService {
    private val _lastScan = MutableStateFlow<NfcScanResult?>(null)
    val lastScan: StateFlow<NfcScanResult?> = _lastScan.asStateFlow()

    private val _scanHistory = MutableStateFlow<List<NfcScanResult>>(emptyList())
    val scanHistory: StateFlow<List<NfcScanResult>> = _scanHistory.asStateFlow()

    private val _writeMode = MutableStateFlow(false)
    val writeMode: StateFlow<Boolean> = _writeMode.asStateFlow()

    private val _pendingWriteData = MutableStateFlow<String?>(null)
    val pendingWriteData: StateFlow<String?> = _pendingWriteData.asStateFlow()

    private val _writeResult = MutableStateFlow<WriteResult?>(null)
    val writeResult: StateFlow<WriteResult?> = _writeResult.asStateFlow()

    private val _isAvailable = MutableStateFlow(false)
    val isAvailable: StateFlow<Boolean> = _isAvailable.asStateFlow()

    /**
     * Non-null when a write-mode scan found existing QuailSync data on the tag
     * and is waiting for user confirmation before overwriting.
     */
    private val _pendingConflict = MutableStateFlow<TagConflict?>(null)
    val pendingConflict: StateFlow<TagConflict?> = _pendingConflict.asStateFlow()

    // Held temporarily between conflict detection and user confirmation.
    // The Tag is still valid because we connected to it in handleIntent.
    private var conflictTag: Tag? = null

    data class WriteResult(val success: Boolean, val message: String)

    /** Result of a write-mode tag scan. */
    sealed class WriteAttemptResult {
        /** Tag was blank or non-QuailSync — written successfully. */
        data class Written(val tagId: String) : WriteAttemptResult()
        /** Tag had QuailSync data — needs user confirmation. */
        data class Conflict(val conflict: TagConflict) : WriteAttemptResult()
        /** Write failed (hardware error). */
        data class Failed(val message: String) : WriteAttemptResult()
    }

    fun checkAvailability(adapter: NfcAdapter?) {
        _isAvailable.value = adapter != null && adapter.isEnabled
    }

    fun enableForegroundDispatch(activity: Activity, adapter: NfcAdapter?) {
        if (adapter == null || !adapter.isEnabled) return
        val intent = Intent(activity, activity.javaClass).apply {
            addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }
        val pendingIntent = PendingIntent.getActivity(
            activity, 0, intent,
            PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val filters = arrayOf(
            IntentFilter(NfcAdapter.ACTION_NDEF_DISCOVERED).apply {
                try { addDataType("text/plain") } catch (_: Exception) {}
            },
            IntentFilter(NfcAdapter.ACTION_TAG_DISCOVERED),
            IntentFilter(NfcAdapter.ACTION_TECH_DISCOVERED),
        )
        val techLists = arrayOf(
            arrayOf(Ndef::class.java.name),
            arrayOf(NdefFormatable::class.java.name),
        )
        try {
            adapter.enableForegroundDispatch(activity, pendingIntent, filters, techLists)
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to enable NFC foreground dispatch", e)
        }
    }

    fun disableForegroundDispatch(activity: Activity, adapter: NfcAdapter?) {
        try {
            adapter?.disableForegroundDispatch(activity)
        } catch (_: Exception) {}
    }

    /**
     * Handles an NFC intent. In write mode, reads the tag first to check for conflicts.
     * Returns a pair of (scan result, write attempt result if in write mode).
     */
    fun handleIntent(intent: Intent): Pair<NfcScanResult, WriteAttemptResult?>? {
        val action = intent.action ?: return null
        if (action != NfcAdapter.ACTION_NDEF_DISCOVERED &&
            action != NfcAdapter.ACTION_TAG_DISCOVERED &&
            action != NfcAdapter.ACTION_TECH_DISCOVERED
        ) return null

        // getParcelableExtra(name) was deprecated in API 33 (Tiramisu) in
        // favour of the typed (name, class) overload. Our minSdk is 26 so we
        // keep the legacy call on older devices and switch on Tiramisu+.
        val tag = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(NfcAdapter.EXTRA_TAG, Tag::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra<Tag>(NfcAdapter.EXTRA_TAG)
        } ?: return null
        val tagId = tag.id.joinToString("") { "%02X".format(it) }
        Log.d("QuailSync", "NFC tag scanned: id=$tagId action=$action")

        val payload = readNdefPayload(intent)
        Log.d("QuailSync", "NFC payload: $payload")

        // If in write mode, check existing data before writing
        if (_writeMode.value && _pendingWriteData.value != null) {
            val writeData = _pendingWriteData.value!!
            val attemptResult = attemptWrite(tag, tagId, payload, writeData)
            val scanResult = NfcScanResult(tagId, payload)

            when (attemptResult) {
                is WriteAttemptResult.Written -> {
                    _writeMode.value = false
                    _pendingWriteData.value = null
                    _writeResult.value = WriteResult(true, "Wrote '$writeData' to tag $tagId")
                    val writtenResult = NfcScanResult(tagId, writeData)
                    _lastScan.value = writtenResult
                    _scanHistory.value = listOf(writtenResult) + _scanHistory.value.take(19)
                    return Pair(writtenResult, attemptResult)
                }
                is WriteAttemptResult.Conflict -> {
                    // Don't clear write mode — we'll resume after user confirms
                    return Pair(scanResult, attemptResult)
                }
                is WriteAttemptResult.Failed -> {
                    _writeResult.value = WriteResult(false, attemptResult.message)
                    // Keep write mode active so user can retry
                    return Pair(scanResult, attemptResult)
                }
            }
        }

        // Normal read mode
        val result = NfcScanResult(tagId, payload)
        _lastScan.value = result
        _scanHistory.value = listOf(result) + _scanHistory.value.take(19)
        return Pair(result, null)
    }

    /**
     * Permissive write path for the batch graduation flow. Bypasses ALL
     * conflict detection — any tag the user taps (blank, formatted-but-empty,
     * carrying a stale `QS-L*`, carrying `BIRD-*` from a prior session, or
     * carrying foreign data) is overwritten with `writeData`. The server's
     * `clear_nfc_tag_from_others` handles DB uniqueness when the eventual
     * bird-create POST runs, so the client never needs to inspect what was
     * on the tag.
     *
     * The batch UX requires every tap to "just work" — the user is holding a
     * chick in one hand and a tag in the other. Pausing for "overwrite?"
     * confirmation, looking up the previous bird, or surfacing a generic
     * "Failed to write" toast all break the flow. This method is the minimal
     * happy-path: extract tag → write → return its hardware uniqueId.
     *
     * On success, write mode is cleared and the tag id is returned. On
     * hardware failure the tag id is null and `_writeResult` is populated
     * with a retry-friendly message that `BatchAwaitingScanScreen`'s banner
     * already renders; write mode is left active so the next tap retries.
     */
    fun handleBatchIntent(intent: Intent, writeData: String): String? {
        val action = intent.action ?: return null
        if (action != NfcAdapter.ACTION_NDEF_DISCOVERED &&
            action != NfcAdapter.ACTION_TAG_DISCOVERED &&
            action != NfcAdapter.ACTION_TECH_DISCOVERED
        ) return null

        val tag = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(NfcAdapter.EXTRA_TAG, Tag::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra<Tag>(NfcAdapter.EXTRA_TAG)
        } ?: return null
        val tagId = tag.id.joinToString("") { "%02X".format(it) }
        Log.d("QuailSync", "Batch write: tag=$tagId payload='$writeData' (no conflict check)")

        if (tryWriteAndRecordSuccess(tag, tagId, writeData)) return tagId

        // First write failed. Some NTAG213/215 tags ship with factory password
        // protection that gates writes even though `isWritable=true` — the
        // first sign is IOException on writeNdefMessage. Try a one-shot
        // unlock with common default passwords; if it works, retry the write
        // within the same tap. This only fires in the batch path because the
        // standalone scanner caller doesn't go through handleBatchIntent.
        if (tag.techList.contains(MifareUltralight::class.java.name)) {
            when (tryRemoveNtagPassword(tag, tagId)) {
                NtagPasswordResult.Removed -> {
                    if (tryWriteAndRecordSuccess(tag, tagId, writeData)) return tagId
                    // Auth worked but the retry still failed — fall through to
                    // generic "tag locked" so we don't lie that it's password-only.
                }
                NtagPasswordResult.CustomPassword -> {
                    _writeResult.value = WriteResult(
                        false,
                        "This tag is password-protected with a custom password. " +
                            "Remove the password using NFC Tools first.",
                    )
                    return null
                }
                NtagPasswordResult.NotApplicable -> {
                    // Either not actually an NTAG variant, or the MU channel
                    // couldn't be opened — generic message below.
                }
            }
        }

        // By the time we get here, writeNdefText has tried both Ndef and
        // NdefFormatable paths AND (if the tech list supports it) we've tried
        // to unlock with default passwords. The tag is unlikely to write on a
        // simple retry. Steer the user to grab a different tag.
        _writeResult.value = WriteResult(
            false,
            "This tag can't be written to — it may be locked. Try a different tag.",
        )
        // Leave write mode active so the next tap (on any tag) retries.
        return null
    }

    /**
     * Run a single NDEF write attempt; on success, mirror the side effects
     * `handleBatchIntent` needs (clear write mode, populate writeResult,
     * append to scan history). Returns whether the write succeeded.
     */
    private fun tryWriteAndRecordSuccess(tag: Tag, tagId: String, writeData: String): Boolean {
        if (!writeNdefText(tag, writeData)) return false
        _writeMode.value = false
        _pendingWriteData.value = null
        _writeResult.value = WriteResult(true, "Wrote '$writeData' to tag $tagId")
        val result = NfcScanResult(tagId, writeData)
        _lastScan.value = result
        _scanHistory.value = listOf(result) + _scanHistory.value.take(19)
        return true
    }

    /** Identifies which NTAG variant a MifareUltralight tag is, so we know
     *  which page holds CFG0 (and therefore AUTH0). */
    private data class NtagVariant(val name: String, val cfg0Page: Int)

    private sealed class NtagPasswordResult {
        /** Authenticated with a default password and AUTH0 set to 0xFF — caller should retry the write. */
        data object Removed : NtagPasswordResult()
        /** Tag is an NTAG but none of the default passwords worked. */
        data object CustomPassword : NtagPasswordResult()
        /** Tag isn't a recognised NTAG variant, or the MU channel failed. */
        data object NotApplicable : NtagPasswordResult()
    }

    /**
     * Best-effort: if the tag is an NTAG with factory password protection
     * still enabled, authenticate with a default password and clear AUTH0 so
     * subsequent NDEF writes succeed.
     *
     * Non-destructive on non-protected tags: an unprotected NTAG either
     * accepts PWD_AUTH and returns PACK (in which case we set AUTH0=0xFF,
     * which is the no-op "no pages need auth" state anyway) or returns NAK,
     * which arrives here as IOException and we move on to the next password.
     */
    private fun tryRemoveNtagPassword(tag: Tag, tagId: String): NtagPasswordResult {
        val mu = MifareUltralight.get(tag) ?: return NtagPasswordResult.NotApplicable
        try {
            mu.connect()
            val variant = detectNtagVariant(mu)
            if (variant == null) {
                Log.w("QuailSync", "NFC: tag $tagId is MifareUltralight but not a recognised NTAG21x — skipping password removal")
                return NtagPasswordResult.NotApplicable
            }
            for (password in DEFAULT_NTAG_PASSWORDS) {
                val authed = try {
                    // PWD_AUTH returns 2-byte PACK on success, 1-byte NAK on
                    // failure. Android's MU.transceive surfaces NAKs as
                    // IOException (TagLostException), so a successful auth
                    // is a response whose size == 2.
                    val response = mu.transceive(byteArrayOf(NTAG_CMD_PWD_AUTH) + password)
                    response.size == 2
                } catch (_: IOException) {
                    false
                }
                if (!authed) continue
                try {
                    // Read CFG0 (4 pages of 4 bytes = 16 bytes; first 4 are CFG0).
                    // Preserve bytes 0..2 (MIRROR_CONF / MIRROR_BYTE / RFUI) so
                    // we only change AUTH0 in byte 3. AUTH0 = 0xFF means "first
                    // page requiring auth is beyond end-of-memory" → no pages
                    // require auth → password protection effectively disabled.
                    val cfg0Page = mu.readPages(variant.cfg0Page)
                    val newCfg0 = byteArrayOf(cfg0Page[0], cfg0Page[1], cfg0Page[2], 0xFF.toByte())
                    mu.writePage(variant.cfg0Page, newCfg0)
                    Log.d(
                        "QuailSync",
                        "NFC: removed password protection from tag $tagId " +
                            "(${variant.name}, AUTH0=0xFF at page ${variant.cfg0Page})",
                    )
                    return NtagPasswordResult.Removed
                } catch (e: IOException) {
                    Log.e(
                        "QuailSync",
                        "NFC: tag $tagId authenticated but failed to clear AUTH0 — tag may have left field",
                        e,
                    )
                    return NtagPasswordResult.NotApplicable
                }
            }
            Log.w(
                "QuailSync",
                "NFC: tag $tagId (${variant.name}) didn't accept any default password — likely a custom password",
            )
            return NtagPasswordResult.CustomPassword
        } catch (e: IOException) {
            Log.e("QuailSync", "NFC: MifareUltralight comm error on tag $tagId", e)
            return NtagPasswordResult.NotApplicable
        } catch (e: Exception) {
            Log.e("QuailSync", "NFC: unexpected error during password removal on tag $tagId", e)
            return NtagPasswordResult.NotApplicable
        } finally {
            try { mu.close() } catch (_: Exception) {}
        }
    }

    /** Issues GET_VERSION (0x60) and decodes the storage_size byte to identify
     *  which NTAG21x variant this is. Returns null if the response is
     *  malformed (e.g., tag isn't an NTAG21x at all, or comms dropped). */
    private fun detectNtagVariant(mu: MifareUltralight): NtagVariant? {
        return try {
            val version = mu.transceive(byteArrayOf(NTAG_CMD_GET_VERSION))
            // GET_VERSION response is 8 bytes: vendor, prod_type, prod_subtype,
            // major_ver, minor_ver, storage_size, protocol_type. Storage size
            // 0x0F = NTAG213 (180 bytes), 0x11 = NTAG215 (540 bytes),
            // 0x13 = NTAG216 (924 bytes). CFG0 lives at different pages on
            // each variant — see NTAG21x datasheet §8.5.
            when (version.getOrNull(6)) {
                0x0F.toByte() -> NtagVariant(name = "NTAG213", cfg0Page = 0x29)
                0x11.toByte() -> NtagVariant(name = "NTAG215", cfg0Page = 0x83)
                0x13.toByte() -> NtagVariant(name = "NTAG216", cfg0Page = 0xE3)
                else -> null
            }
        } catch (_: IOException) {
            null
        }
    }

    /**
     * Reads the tag, checks for QuailSync data, and either writes or returns a conflict.
     */
    private fun attemptWrite(
        tag: Tag,
        tagId: String,
        existingPayload: String?,
        writeData: String,
    ): WriteAttemptResult {
        // Check if tag has existing QuailSync data
        if (existingPayload != null && existingPayload.startsWith("BIRD-")) {
            val existingBirdId = existingPayload.removePrefix("BIRD-").toIntOrNull()
            if (existingBirdId != null) {
                // Don't write if it's the same data we're trying to write
                if (existingPayload == writeData) {
                    // Tag already has the correct data — treat as success
                    return WriteAttemptResult.Written(tagId)
                }
                // Conflict — need user confirmation
                val conflict = TagConflict(
                    tagId = tagId,
                    existingPayload = existingPayload,
                    existingBirdId = existingBirdId,
                    pendingWriteData = writeData,
                )
                _pendingConflict.value = conflict
                conflictTag = tag
                Log.d("QuailSync", "Tag conflict: existing=$existingPayload, pending=$writeData")
                return WriteAttemptResult.Conflict(conflict)
            }
        }

        // Blank or non-QuailSync data — write immediately
        val success = writeNdefText(tag, writeData)
        return if (success) {
            WriteAttemptResult.Written(tagId)
        } else {
            WriteAttemptResult.Failed("Failed to write to tag")
        }
    }

    /**
     * User confirmed overwriting a conflicting tag. Write the pending data now.
     * Returns true if the write succeeded.
     * Note: the Tag object may be stale if the user took too long to confirm.
     */
    fun confirmOverwrite(): Boolean {
        val conflict = _pendingConflict.value ?: return false
        val tag = conflictTag

        _pendingConflict.value = null

        if (tag == null) {
            _writeResult.value = WriteResult(false, "Tag lost — tap the tag again")
            // Keep write mode active for retry
            return false
        }

        val success = writeNdefText(tag, conflict.pendingWriteData)
        conflictTag = null

        if (success) {
            _writeMode.value = false
            _pendingWriteData.value = null
            _writeResult.value = WriteResult(true, "Wrote '${conflict.pendingWriteData}' to tag ${conflict.tagId}")
            val result = NfcScanResult(conflict.tagId, conflict.pendingWriteData)
            _lastScan.value = result
            _scanHistory.value = listOf(result) + _scanHistory.value.take(19)
        } else {
            _writeResult.value = WriteResult(false, "Tag lost — tap the tag again")
            // Keep write mode active for retry
        }
        return success
    }

    /**
     * User cancelled overwriting — discard the conflict but keep write mode active
     * so they can tap a different tag.
     */
    fun cancelOverwrite() {
        _pendingConflict.value = null
        conflictTag = null
        // Write mode stays active — user should tap a different tag
    }

    fun enterWriteMode(data: String) {
        _pendingWriteData.value = data
        _writeMode.value = true
        _writeResult.value = null
        _pendingConflict.value = null
        conflictTag = null
    }

    fun cancelWriteMode() {
        _writeMode.value = false
        _pendingWriteData.value = null
        _pendingConflict.value = null
        conflictTag = null
    }

    fun setWriteResult(result: WriteResult) {
        _writeResult.value = result
    }

    fun clearWriteResult() {
        _writeResult.value = null
    }

    fun updateScanWithBird(tagId: String, bird: Bird) {
        val current = _lastScan.value
        if (current != null && current.tagId == tagId) {
            _lastScan.value = current.copy(bird = bird)
        }
        _scanHistory.value = _scanHistory.value.map { scan ->
            if (scan.tagId == tagId && scan.bird == null) scan.copy(bird = bird)
            else scan
        }
    }

    private fun readNdefPayload(intent: Intent): String? {
        // Same Tiramisu deprecation story as getParcelableExtra above —
        // typed overload only exists on API 33+. Array<NdefMessage> on the
        // new path projects cleanly to Array<out Parcelable> via Kotlin's
        // `out` variance, so no cast or unchecked-suppression is needed
        // to unify the two branches.
        val rawMessages: Array<out Parcelable>? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableArrayExtra(NfcAdapter.EXTRA_NDEF_MESSAGES, NdefMessage::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableArrayExtra(NfcAdapter.EXTRA_NDEF_MESSAGES)
        }
        rawMessages ?: return null
        return rawMessages.filterIsInstance<NdefMessage>()
            .flatMap { it.records.toList() }
            .firstOrNull { it.tnf == NdefRecord.TNF_WELL_KNOWN || it.tnf == NdefRecord.TNF_MIME_MEDIA }
            ?.let { record ->
                val payload = record.payload
                if (record.tnf == NdefRecord.TNF_WELL_KNOWN &&
                    record.type.contentEquals(NdefRecord.RTD_TEXT)
                ) {
                    val languageCodeLength = payload[0].toInt() and 0x3F
                    String(payload, 1 + languageCodeLength, payload.size - 1 - languageCodeLength, Charsets.UTF_8)
                } else {
                    String(payload, Charsets.UTF_8)
                }
            }
    }

    /**
     * Writes `text` to the tag as an NDEF text record. Handles two tag states:
     *  - **Already NDEF-formatted** (`Ndef.get(tag) != null`): connect and
     *    write the message directly. This is the common case once a tag has
     *    been programmed at least once.
     *  - **Blank / un-formatted** (`Ndef.get(tag) == null` but
     *    `NdefFormatable.get(tag) != null`): the factory state for most fresh
     *    NTAG / Mifare tags. `format(message)` writes the NDEF capability
     *    container AND the message in one round-trip, so we never need a
     *    separate "format then write" second tap.
     *
     * Returns `false` on hardware failure, read-only tag, or an unsupported
     * tag tech. The caller surfaces the message in `_writeResult` so the
     * batch screen's banner can guide the user.
     */
    private fun writeNdefText(tag: Tag, text: String): Boolean {
        val record = NdefRecord.createTextRecord("en", text)
        val message = NdefMessage(arrayOf(record))
        val tagId = tag.id.joinToString("") { "%02X".format(it) }

        // Try the Ndef (already-formatted) path first. Any failure mode —
        // read-only, IOException, unexpected exception — falls through to
        // the NdefFormatable path: some tags that report `isWritable=false`
        // via Ndef can still be reformatted (the OTP/lock bits on certain
        // NTAG variants gate the data area but not the capability
        // container), and a transient IOException on the Ndef channel
        // doesn't necessarily mean the formatable channel is also dead.
        val ndef = Ndef.get(tag)
        if (ndef != null && writeToFormattedNdef(ndef, message, text, tagId)) return true

        // Fallback (also covers the truly-blank tag case, where
        // Ndef.get(tag) returns null from the start).
        if (writeToBlankTag(tag, message, text, tagId)) return true

        Log.e(
            "QuailSync",
            "Tag $tagId is not writable by any method — tag may be locked or defective. " +
                "techs=${tag.techList.joinToString(",")}",
        )
        return false
    }

    /** Path for tags already carrying an NDEF capability container. */
    private fun writeToFormattedNdef(ndef: Ndef, message: NdefMessage, text: String, tagId: String): Boolean {
        return try {
            ndef.connect()
            // Diagnostics: surface the tag's reported state so a locked-tag
            // report from the user is debuggable from logs alone. IOException
            // on writeNdefMessage gives no hint about why — printing isWritable,
            // maxSize, type, and isConnected here lets us tell "locked"
            // (isWritable=false) from "tag too small" (maxSize<payload) from
            // "channel dropped" (isConnected=false at write time).
            Log.d(
                "QuailSync",
                "NFC tag $tagId: writable=${ndef.isWritable}, maxSize=${ndef.maxSize}, " +
                    "type=${ndef.type}, connected=${ndef.isConnected}",
            )
            if (!ndef.isWritable) {
                Log.w("QuailSync", "NFC tag $tagId is read-only via Ndef path — falling back to NdefFormatable")
                ndef.close()
                return false
            }
            ndef.writeNdefMessage(message)
            ndef.close()
            Log.d("QuailSync", "NFC write success (Ndef path) for tag $tagId: $text")
            true
        } catch (e: IOException) {
            Log.e("QuailSync", "NFC write IOException (Ndef path) for tag $tagId — may be locked or out of field", e)
            try { ndef.close() } catch (_: Exception) {}
            false
        } catch (e: Exception) {
            Log.e("QuailSync", "NFC write error (Ndef path) for tag $tagId", e)
            try { ndef.close() } catch (_: Exception) {}
            false
        }
    }

    /**
     * Path for blank or fallback-after-Ndef-failure: try NdefFormatable, which
     * writes the capability container and the message atomically. Used both
     * for truly-blank tags (factory state) and as a last-resort retry when
     * the Ndef path reports read-only or throws IOException.
     */
    private fun writeToBlankTag(tag: Tag, message: NdefMessage, text: String, tagId: String): Boolean {
        val formatable = NdefFormatable.get(tag)
        if (formatable == null) {
            Log.w(
                "QuailSync",
                "Tag $tagId doesn't support NdefFormatable — techs=${tag.techList.joinToString(",")}",
            )
            return false
        }
        return try {
            formatable.connect()
            formatable.format(message)
            formatable.close()
            Log.d("QuailSync", "NFC format+write success (NdefFormatable path) for tag $tagId: $text")
            true
        } catch (e: IOException) {
            Log.e("QuailSync", "NFC format IOException (NdefFormatable path) for tag $tagId", e)
            try { formatable.close() } catch (_: Exception) {}
            false
        } catch (e: Exception) {
            Log.e("QuailSync", "NFC format error (NdefFormatable path) for tag $tagId", e)
            try { formatable.close() } catch (_: Exception) {}
            false
        }
    }
}
