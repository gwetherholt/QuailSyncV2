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
import java.time.format.DateTimeFormatter

data class NfcScanResult(
    val tagId: String,
    val payload: String?,
    val timestamp: LocalDateTime = LocalDateTime.now(),
    val bird: Bird? = null,
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

    data class WriteResult(val success: Boolean, val message: String)

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

    fun handleIntent(intent: Intent): NfcScanResult? {
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

        // If in write mode, write to the tag instead
        if (_writeMode.value && _pendingWriteData.value != null) {
            val writeData = _pendingWriteData.value!!
            val success = writeNdefText(tag, writeData)
            _writeResult.value = if (success) {
                WriteResult(true, "Wrote '$writeData' to tag $tagId")
            } else {
                WriteResult(false, "Failed to write to tag")
            }
            _writeMode.value = false
            _pendingWriteData.value = null
            if (success) {
                // Return scan result with the written data
                val result = NfcScanResult(tagId, writeData)
                _lastScan.value = result
                _scanHistory.value = listOf(result) + _scanHistory.value.take(19)
                return result
            }
            return null
        }

        val result = NfcScanResult(tagId, payload)
        _lastScan.value = result
        _scanHistory.value = listOf(result) + _scanHistory.value.take(19)
        return result
    }

    fun enterWriteMode(data: String) {
        _pendingWriteData.value = data
        _writeMode.value = true
        _writeResult.value = null
    }

    fun cancelWriteMode() {
        _writeMode.value = false
        _pendingWriteData.value = null
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
                    // NDEF text record: first byte is status (encoding + language length)
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
                // Try to format the tag
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
