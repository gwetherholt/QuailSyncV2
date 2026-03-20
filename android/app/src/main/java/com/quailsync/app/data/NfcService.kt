package com.quailsync.app.data

import android.app.Activity
import android.app.PendingIntent
import android.content.Intent
import android.content.IntentFilter
import android.nfc.NdefMessage
import android.nfc.NdefRecord
import android.nfc.NfcAdapter
import android.nfc.Tag
import android.nfc.tech.Ndef
import android.nfc.tech.NdefFormatable
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

        val tag = intent.getParcelableExtra<Tag>(NfcAdapter.EXTRA_TAG) ?: return null
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
        val rawMessages = intent.getParcelableArrayExtra(NfcAdapter.EXTRA_NDEF_MESSAGES)
            ?: return null
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

    private fun writeNdefText(tag: Tag, text: String): Boolean {
        val record = NdefRecord.createTextRecord("en", text)
        val message = NdefMessage(arrayOf(record))

        return try {
            val ndef = Ndef.get(tag)
            if (ndef != null) {
                ndef.connect()
                if (!ndef.isWritable) {
                    Log.e("QuailSync", "NFC tag is read-only")
                    ndef.close()
                    return false
                }
                ndef.writeNdefMessage(message)
                ndef.close()
                Log.d("QuailSync", "NFC write success: $text")
                true
            } else {
                val formatable = NdefFormatable.get(tag)
                if (formatable != null) {
                    formatable.connect()
                    formatable.format(message)
                    formatable.close()
                    Log.d("QuailSync", "NFC format+write success: $text")
                    true
                } else {
                    Log.e("QuailSync", "Tag doesn't support NDEF")
                    false
                }
            }
        } catch (e: IOException) {
            Log.e("QuailSync", "NFC write error", e)
            false
        } catch (e: Exception) {
            Log.e("QuailSync", "NFC write error", e)
            false
        }
    }
}
