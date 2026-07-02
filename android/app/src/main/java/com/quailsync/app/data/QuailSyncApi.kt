package com.quailsync.app.data

import com.google.gson.annotations.SerializedName
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.logging.HttpLoggingInterceptor
import retrofit2.Retrofit
import retrofit2.converter.gson.GsonConverterFactory
import retrofit2.http.Body
import retrofit2.http.GET
import retrofit2.http.Multipart
import retrofit2.http.DELETE
import retrofit2.http.POST
import retrofit2.http.PUT
import retrofit2.http.Part
import retrofit2.http.Path

data class Brooder(
    @SerializedName("id") val id: Int,
    @SerializedName("name") val name: String,
    @SerializedName("location") val location: String? = null,
    @SerializedName("capacity") val capacity: Int? = null,
    @SerializedName("status") val status: String? = null,
    @SerializedName("latest_temperature") val latestTemperature: Double? = null,
    @SerializedName("latest_humidity") val latestHumidity: Double? = null,
    // Alternative field names the server might use
    @SerializedName("latest_temperature_f") val latestTemperatureF: Double? = null,
    @SerializedName("latest_humidity_percent") val latestHumidityPercent: Double? = null,
    @SerializedName("camera_url") val cameraUrl: String? = null,
    @SerializedName("qr_code") val qrCode: String? = null,
    @SerializedName("lineage_id") val lineageId: Int? = null,
    @SerializedName("life_stage") val lifeStage: String? = null,
    /** Housing axis (issue #11): "incubator" / "brooder" / "hutch". Default
     *  "brooder" for rows created before the migration. */
    @SerializedName("housing_type") val housingType: String? = "brooder",
)

data class UpdateBrooderRequest(
    @SerializedName("camera_url") val cameraUrl: String? = null,
    @SerializedName("housing_type") val housingType: String? = null,
)

data class CreateCameraRequest(
    @SerializedName("name") val name: String,
    @SerializedName("feed_url") val feedUrl: String,
    @SerializedName("location") val location: String? = null,
)

data class BrooderReading(
    @SerializedName("id") val id: Int? = null,
    @SerializedName("brooder_id") val brooderId: Int,
    @SerializedName("temperature_f") val temperature: Double?,
    @SerializedName("humidity_percent") val humidity: Double?,
    @SerializedName("recorded_at") val recordedAt: String? = null,
)

data class BrooderAlert(
    @SerializedName("id") val id: Int? = null,
    @SerializedName("brooder_id") val brooderId: Int,
    @SerializedName("alert_type") val alertType: String,
    @SerializedName("severity") val severity: String? = null,
    @SerializedName("message") val message: String? = null,
    @SerializedName("acknowledged") val acknowledged: Boolean? = false,
    @SerializedName("created_at") val createdAt: String? = null,
)

data class Bird(
    @SerializedName("id") val id: Int,
    @SerializedName("band_id") val bandId: String? = null,
    @SerializedName("band_color") val bandColor: String? = null,
    @SerializedName("species") val species: String? = null,
    @SerializedName("sex") val sex: String? = null,
    @SerializedName("status") val status: String? = null,
    @SerializedName("hatch_date") val hatchDate: String? = null,
    @SerializedName("brooder_id") val brooderId: Int? = null,
    @SerializedName("notes") val notes: String? = null,
    @SerializedName("sire_id") val sireId: Int? = null,
    @SerializedName("dam_id") val damId: Int? = null,
    @SerializedName("latest_weight") val latestWeight: Double? = null,
    @SerializedName("nfc_tag_id") val nfcTagId: String? = null,
    /** Issue #13: permanent housing assignment for adult birds. `null` = unhoused. */
    @SerializedName("housing_id") val housingId: Int? = null,
    /** Issue #14: back-link to the chick group the bird graduated from.
     *  Populated by the server (`/graduate` handler) or by the batch-band
     *  flow when it passes chick_group_id on POST /api/birds. */
    @SerializedName("chick_group_id") val chickGroupId: Int? = null,
    /** Many-to-many lineages, populated by the server from the junction table. */
    @SerializedName("lineages") val lineages: List<Lineage> = emptyList(),
) {
    // The two shims below are deliberately kept around for any out-of-tree
    // callers that might still read them. `@Suppress("unused")` silences the
    // compiler's "property never used" warning — the `@Deprecated` marker
    // still surfaces in the IDE for any caller that reaches for them.
    @Suppress("unused")
    @Deprecated(
        "Birds now have many-to-many lineages — use `bird.lineages` and `formatLineages(bird.lineages)` instead. This shim returns only the first lineage's id and hides multi-lineage data from the UI.",
        ReplaceWith("lineages.firstOrNull()?.id"),
    )
    val lineageId: Int? get() = lineages.firstOrNull()?.id

    @Suppress("unused")
    @Deprecated(
        "Birds now have many-to-many lineages — use `formatLineages(bird.lineages)` for display.",
        ReplaceWith("lineages.firstOrNull()?.name"),
    )
    val lineageName: String? get() = lineages.firstOrNull()?.name
}

data class BirdWeight(
    @SerializedName("id") val id: Int? = null,
    @SerializedName("bird_id") val birdId: Int,
    @SerializedName("weight_grams") val weightGrams: Double,
    @SerializedName("date") val date: String? = null,
    @SerializedName("notes") val notes: String? = null,
)

data class CreateWeightRequest(
    @SerializedName("weight_grams") val weightGrams: Double,
    @SerializedName("date") val date: String,
    @SerializedName("notes") val notes: String? = null,
)

data class Lineage(
    @SerializedName("id") val id: Int,
    @SerializedName("name") val name: String,
    @SerializedName("source") val source: String? = null,
    @SerializedName("notes") val notes: String? = null,
)

/**
 * Single-line label for a list of lineages, with truncation at [maxShown].
 *
 * Rules:
 *  - 0 lineages → [emptyText] (default `(no lineage)`).
 *  - 1..maxShown → comma-separated names, e.g. "Fernbank, NWQuail".
 *  - >maxShown   → first maxShown names, then " +N", e.g. "A, B, C +2".
 *
 * Cards that need the *full* list (long-press, detail screen) should iterate
 * over `lineages` directly rather than calling this.
 */
fun formatLineages(
    lineages: List<Lineage>,
    maxShown: Int = 3,
    emptyText: String = "(no lineage)",
): String {
    if (lineages.isEmpty()) return emptyText
    if (lineages.size <= maxShown) return lineages.joinToString(", ") { it.name }
    val head = lineages.take(maxShown).joinToString(", ") { it.name }
    return "$head +${lineages.size - maxShown}"
}

data class CreateLineageRequest(
    @SerializedName("name") val name: String,
    @SerializedName("source") val source: String = "",
    @SerializedName("notes") val notes: String? = null,
)

data class Clutch(
    @SerializedName("id") val id: Int,
    /** Breeding group that produced the eggs (null for lineage-only clutches).
     *  `breedingGroupName` is JOINed in by the server so no extra lookup needed. */
    @SerializedName("breeding_group_id") val breedingGroupId: Int? = null,
    @SerializedName("breeding_group_name") val breedingGroupName: String? = null,
    @SerializedName("lineage_id") val lineageId: Int? = null,
    @SerializedName("lineage_name") val lineageName: String? = null,
    @SerializedName("egg_count") val eggCount: Int? = null,
    @SerializedName("eggs_set") val eggsSet: Int? = null,
    @SerializedName("fertile_count") val fertileCount: Int? = null,
    @SerializedName("eggs_fertile") val eggsFertile: Int? = null,
    @SerializedName("hatch_count") val hatchCount: Int? = null,
    @SerializedName("eggs_hatched") val eggsHatched: Int? = null,
    @SerializedName("set_date") val setDate: String? = null,
    @SerializedName("expected_hatch_date") val expectedHatchDate: String? = null,
    @SerializedName("status") val status: String? = null,
    @SerializedName("notes") val notes: String? = null,
    @SerializedName("eggs_stillborn") val eggsStillborn: Int? = null,
    @SerializedName("eggs_quit") val eggsQuit: Int? = null,
    @SerializedName("eggs_infertile") val eggsInfertile: Int? = null,
    @SerializedName("eggs_damaged") val eggsDamaged: Int? = null,
    @SerializedName("hatch_notes") val hatchNotes: String? = null,
) {
    val totalEggs: Int? get() = eggsSet ?: eggCount
    val totalFertile: Int? get() = eggsFertile ?: fertileCount
    val totalHatched: Int? get() = eggsHatched ?: hatchCount
}

data class CreateClutchRequest(
    /** Optional breeding group that produced the eggs. Lineage-only clutches still work. */
    @SerializedName("breeding_group_id") val breedingGroupId: Int? = null,
    @SerializedName("lineage_id") val lineageId: Int? = null,
    @SerializedName("eggs_set") val eggsSet: Int,
    @SerializedName("set_date") val setDate: String,
    @SerializedName("status") val status: String = "Incubating",
    @SerializedName("notes") val notes: String? = null,
)

data class UpdateClutchRequest(
    @SerializedName("eggs_fertile") val eggsFertile: Int? = null,
    @SerializedName("eggs_hatched") val eggsHatched: Int? = null,
    @SerializedName("status") val status: String? = null,
    @SerializedName("notes") val notes: String? = null,
    @SerializedName("set_date") val setDate: String? = null,
    @SerializedName("eggs_stillborn") val eggsStillborn: Int? = null,
    @SerializedName("eggs_quit") val eggsQuit: Int? = null,
    @SerializedName("eggs_infertile") val eggsInfertile: Int? = null,
    @SerializedName("eggs_damaged") val eggsDamaged: Int? = null,
    @SerializedName("hatch_notes") val hatchNotes: String? = null,
)

data class CreateChickGroupRequest(
    @SerializedName("clutch_id") val clutchId: Int? = null,
    /** Many-to-many lineage IDs. Must contain at least one — server returns 400 otherwise. */
    @SerializedName("lineage_ids") val lineageIds: List<Int>,
    @SerializedName("brooder_id") val brooderId: Int? = null,
    @SerializedName("initial_count") val initialCount: Int,
    @SerializedName("hatch_date") val hatchDate: String,
    @SerializedName("notes") val notes: String? = null,
)

data class ReplaceLineagesRequest(
    @SerializedName("lineage_ids") val lineageIds: List<Int>,
)

/** Body for POST /api/brooders/{id}/assign-birds and /unassign-birds (issue #13). */
data class BirdAssignmentRequest(
    @SerializedName("bird_ids") val birdIds: List<Int>,
)

data class BirdAssignmentResponse(
    @SerializedName("updated") val updated: Long,
)

/** Body for POST /api/brooders/{id}/assign-graduated-group (issue #14). */
data class AssignGraduatedGroupRequest(
    @SerializedName("group_id") val groupId: Int,
)

data class AssignGraduatedGroupResponse(
    @SerializedName("group_id") val groupId: Int,
    @SerializedName("housing_id") val housingId: Int,
    @SerializedName("birds_updated") val birdsUpdated: Long,
)

data class Camera(
    @SerializedName("id") val id: Int,
    @SerializedName("name") val name: String,
    @SerializedName("url") val url: String? = null,
    @SerializedName("feed_url") val feedUrl: String? = null,
    @SerializedName("location") val location: String? = null,
    @SerializedName("status") val status: String? = null,
    @SerializedName("brooder_id") val brooderId: Int? = null,
    @SerializedName("brooder_name") val brooderName: String? = null,
)

data class UpdateBirdRequest(
    @SerializedName("status") val status: String? = null,
    @SerializedName("notes") val notes: String? = null,
    @SerializedName("nfc_tag_id") val nfcTagId: String? = null,
    /** Newly-editable post-banding fields. `null` means "leave unchanged". */
    @SerializedName("band_color") val bandColor: String? = null,
    @SerializedName("sex") val sex: String? = null,
    @SerializedName("hatch_date") val hatchDate: String? = null,
)

data class CreateBirdRequest(
    @SerializedName("band_color") val bandColor: String? = null,
    @SerializedName("sex") val sex: String = "Unknown",
    /** Many-to-many lineage IDs. Must contain at least one. */
    @SerializedName("lineage_ids") val lineageIds: List<Long>,
    @SerializedName("hatch_date") val hatchDate: String,
    @SerializedName("mother_id") val motherId: Long? = null,
    @SerializedName("father_id") val fatherId: Long? = null,
    @SerializedName("generation") val generation: Int = 1,
    @SerializedName("status") val status: String = "Active",
    @SerializedName("notes") val notes: String? = null,
    @SerializedName("nfc_tag_id") val nfcTagId: String? = null,
    /** Issue #14: back-link to the chick group this bird is graduating from.
     *  Server-side `/graduate` stamps this internally; the batch-band flow
     *  uses individual POST /api/birds calls and needs to pass it explicitly
     *  so "Assign Graduated Group" can later find every bird in a group. */
    @SerializedName("chick_group_id") val chickGroupId: Int? = null,
)

data class PhotoUploadResponse(
    @SerializedName("id") val id: Int? = null,
    @SerializedName("url") val url: String? = null,
    @SerializedName("path") val path: String? = null,
)

/** One entry in a bird's photo history (GET /api/birds/{id}/photos). `url` is
 *  server-relative (e.g. "/api/birds/7/photos/bird_7_...jpg") — prepend the
 *  configured server base URL before loading. */
data class BirdPhoto(
    @SerializedName("filename") val filename: String,
    @SerializedName("uploaded_at") val uploadedAt: String,
    @SerializedName("url") val url: String,
)

/** Latest outdoor-camera observation (GET /api/trailcam/latest/{camera_id}).
 *  `imageUrl`/`annotatedImageUrl` are server-relative — prepend the configured
 *  server base URL. `annotatedImageUrl` (bounding boxes drawn on) is null when
 *  no annotated copy exists; fall back to `imageUrl` then. */
data class TrailcamLatest(
    @SerializedName("camera_id") val cameraId: String? = null,
    @SerializedName("bird_count") val birdCount: Int? = null,
    @SerializedName("timestamp") val timestamp: String? = null,
    @SerializedName("confidence_avg") val confidenceAvg: Double? = null,
    @SerializedName("image_url") val imageUrl: String? = null,
    @SerializedName("annotated_image_url") val annotatedImageUrl: String? = null,
    @SerializedName("detections") val detections: List<TrailcamDetection> = emptyList(),
)

data class TrailcamDetection(
    @SerializedName("class_name") val className: String? = null,
    @SerializedName("confidence") val confidence: Double? = null,
    @SerializedName("bbox") val bbox: List<Double> = emptyList(),
)

/** A distinct outdoor camera (GET /api/trailcam/cameras), labelled by the
 *  server in order of first appearance ("Outdoor Cam 1", …). */
data class TrailcamCamera(
    @SerializedName("camera_id") val cameraId: String,
    @SerializedName("label") val label: String,
)

// --- Govee H5179 temp/humidity sensors -------------------------------------

/** A registered Govee sensor with its current assignment and latest reading
 *  (GET /api/govee/sensors). `assignment`/`latestReading` are null when the
 *  sensor is unassigned / hasn't reported yet. */
data class GoveeSensorDto(
    @SerializedName("id") val id: Int,
    @SerializedName("govee_device_id") val goveeDeviceId: String,
    @SerializedName("name") val name: String? = null,
    @SerializedName("model") val model: String? = null,
    @SerializedName("first_seen") val firstSeen: String? = null,
    @SerializedName("last_seen") val lastSeen: String? = null,
    @SerializedName("assignment") val assignment: GoveeAssignmentDto? = null,
    @SerializedName("latest_reading") val latestReading: GoveeLatestReadingDto? = null,
)

data class GoveeAssignmentDto(
    @SerializedName("brooder_id") val brooderId: Int,
    @SerializedName("brooder_name") val brooderName: String,
    @SerializedName("assigned_at") val assignedAt: String? = null,
)

data class GoveeLatestReadingDto(
    @SerializedName("temperature_f") val temperatureF: Double,
    @SerializedName("humidity") val humidity: Double,
    @SerializedName("recorded_at") val recordedAt: String? = null,
)

/** Body of PUT /api/govee/sensors/{id}/assign. */
data class AssignSensorRequest(
    @SerializedName("brooder_id") val brooderId: Int,
)

/** A registered SPYPOINT trail camera with its current assignment
 *  (GET /api/trail-cameras). `assignment` is null when unassigned. */
data class TrailCameraDto(
    @SerializedName("id") val id: Int,
    @SerializedName("spypoint_camera_id") val spypointCameraId: String,
    @SerializedName("name") val name: String? = null,
    @SerializedName("model") val model: String? = null,
    @SerializedName("first_seen") val firstSeen: String? = null,
    @SerializedName("last_seen") val lastSeen: String? = null,
    @SerializedName("assignment") val assignment: CameraAssignmentDto? = null,
)

data class CameraAssignmentDto(
    @SerializedName("brooder_id") val brooderId: Int,
    @SerializedName("brooder_name") val brooderName: String,
    @SerializedName("assigned_at") val assignedAt: String? = null,
)

/** Body of PUT /api/trail-cameras/{id}/assign. */
data class AssignCameraRequest(
    @SerializedName("brooder_id") val brooderId: Int,
)

// --- Indoor cameras (RTSP chick-counter) -----------------------------------

/** Latest indoor-camera observation (GET /api/indoorcam/latest/{camera_id}).
 *  `imageUrl`/`annotatedImageUrl` are server-relative — prepend the configured
 *  server base URL. Most observations carry NO image (null): only "notable"
 *  frames are saved, and those may be cleared after a Roboflow upload. */
data class IndoorcamLatest(
    @SerializedName("camera_id") val cameraId: String? = null,
    @SerializedName("detection_count") val detectionCount: Int? = null,
    @SerializedName("timestamp") val timestamp: String? = null,
    @SerializedName("confidence_avg") val confidenceAvg: Double? = null,
    @SerializedName("image_url") val imageUrl: String? = null,
    @SerializedName("annotated_image_url") val annotatedImageUrl: String? = null,
    @SerializedName("detections") val detections: List<TrailcamDetection> = emptyList(),
)

/** A registered indoor camera with its current assignment (GET
 *  /api/indoor-cameras). Indoor cameras only watch brooders/incubators. */
data class IndoorCamera(
    @SerializedName("id") val id: Int,
    @SerializedName("camera_id") val cameraId: String,
    @SerializedName("name") val name: String? = null,
    @SerializedName("rtsp_url") val rtspUrl: String? = null,
    @SerializedName("model") val model: String? = null,
    @SerializedName("first_seen") val firstSeen: String? = null,
    @SerializedName("last_seen") val lastSeen: String? = null,
    @SerializedName("created_at") val createdAt: String? = null,
    @SerializedName("assignment") val assignment: IndoorCameraAssignment? = null,
)

/** Current assignment of an indoor camera. `housingType` is "brooder" or
 *  "incubator" (never "hutch" — the server rejects that). */
data class IndoorCameraAssignment(
    @SerializedName("brooder_id") val brooderId: Int,
    @SerializedName("brooder_name") val brooderName: String,
    @SerializedName("housing_type") val housingType: String? = null,
    @SerializedName("assigned_at") val assignedAt: String? = null,
)

/** Body of PUT /api/indoor-cameras/{id}/assign. */
data class AssignIndoorCameraRequest(
    @SerializedName("brooder_id") val brooderId: Int,
)

data class TargetTempResponse(
    @SerializedName("brooder_id") val brooderId: Int,
    @SerializedName("target_temp_f") val targetTempF: Double,
    @SerializedName("min_temp_f") val minTempF: Double,
    @SerializedName("max_temp_f") val maxTempF: Double,
    @SerializedName("week") val week: Int,
    @SerializedName("age_days") val ageDays: Int?,
    @SerializedName("chick_group_id") val chickGroupId: Int?,
    @SerializedName("schedule_label") val scheduleLabel: String,
    @SerializedName("status") val status: String,
)

data class HeadcountResponse(
    @SerializedName("brooder_id") val brooderId: Int? = null,
    @SerializedName("count") val count: Int? = null,
    @SerializedName("timestamp") val timestamp: String? = null,
)

data class ChickGroupDto(
    @SerializedName("id") val id: Int,
    @SerializedName("clutch_id") val clutchId: Int? = null,
    @SerializedName("brooder_id") val brooderId: Int? = null,
    @SerializedName("initial_count") val initialCount: Int,
    @SerializedName("current_count") val currentCount: Int,
    @SerializedName("hatch_date") val hatchDate: String,
    @SerializedName("status") val status: String,
    @SerializedName("notes") val notes: String? = null,
    /** Issue #14: which hutch the graduated group lives in. Null for Active
     *  groups (still in nursery `brooderId`) and graduated groups that
     *  haven't been placed yet. */
    @SerializedName("housing_id") val housingId: Int? = null,
    @SerializedName("is_ready_to_transition") val isReadyToTransition: Boolean = false,
    /** Many-to-many lineages, populated by the server from the junction table. */
    @SerializedName("lineages") val lineages: List<Lineage> = emptyList(),
) {
    // Kept as a back-compat shim; `@Suppress("unused")` silences the compiler
    // since every in-tree caller was migrated off these.
    @Suppress("unused")
    @Deprecated(
        "Chick groups now have many-to-many lineages — use `group.lineages` and `formatLineages(group.lineages)` instead.",
        ReplaceWith("lineages.firstOrNull()?.id"),
    )
    val lineageId: Int? get() = lineages.firstOrNull()?.id

    @Suppress("unused")
    @Deprecated(
        "Use `formatLineages(group.lineages)` which handles truncation for 4+ lineages.",
        ReplaceWith("formatLineages(lineages)", "com.quailsync.app.data.formatLineages"),
    )
    val lineageNames: String get() = lineages.joinToString(", ") { it.name }
}


data class AssignGroupRequest(
    @SerializedName("group_id") val groupId: Int,
)

data class BrooderResidentsResponse(
    @SerializedName("brooder_id") val brooderId: Int,
    @SerializedName("chick_groups") val chickGroups: List<ChickGroupDto>,
    @SerializedName("individual_birds") val individualBirds: List<Bird>,
)

data class MoveBirdRequest(
    @SerializedName("target_brooder_id") val targetBrooderId: Int?,
)

// Breeding & Culling models

data class BreedingGroupDto(
    @SerializedName("id") val id: Int,
    @SerializedName("name") val name: String,
    @SerializedName("male_ids") val maleIds: List<Int> = emptyList(),
    @SerializedName("female_ids") val femaleIds: List<Int> = emptyList(),
    @SerializedName("start_date") val startDate: String? = null,
    @SerializedName("notes") val notes: String? = null,
    /** "active" (>=1 male) or "infertile" (no males). */
    @SerializedName("status") val status: String = "active",
) {
    /** Full male roster — the junction is the only source of truth. */
    val males: List<Int> get() = maleIds
    val isInfertile: Boolean get() = status == "infertile" || maleIds.isEmpty()
}

data class CreateBreedingGroupRequest(
    @SerializedName("name") val name: String,
    @SerializedName("male_ids") val maleIds: List<Int>,
    @SerializedName("female_ids") val femaleIds: List<Int>,
    @SerializedName("start_date") val startDate: String,
    @SerializedName("notes") val notes: String? = null,
)

/** Partial edit; null fields are left unchanged server-side. */
data class UpdateBreedingGroupRequest(
    @SerializedName("name") val name: String? = null,
    @SerializedName("male_ids") val maleIds: List<Int>? = null,
    @SerializedName("female_ids") val femaleIds: List<Int>? = null,
    @SerializedName("notes") val notes: String? = null,
)

// ---------------------------------------------------------------------------
// Dropped-tag reconciliation ("whose band is this?"). Read-only deduction —
// the server never writes a band assignment. See docs/dropped_tag_deduction.md.
// ---------------------------------------------------------------------------

data class ReconcileRequest(
    /** Tags found on the floor; their stored bird records are what we look for. */
    @SerializedName("orphan_tag_ids") val orphanTagIds: List<String>,
    /** Present unbanded birds, described by observation. */
    @SerializedName("observed_birds") val observedBirds: List<ObservedBirdDto>,
)

data class ObservedBirdDto(
    /** Client-side handle echoed back in the result; NOT a DB id. */
    @SerializedName("ref_id") val refId: String,
    /** "Male" | "Female" | "Unknown" | null. Both null and "Unknown" mean
     *  "not sure" and never eliminate a candidate. */
    @SerializedName("sex") val sex: String? = null,
    @SerializedName("bloodline") val bloodline: String? = null,
    @SerializedName("traits") val traits: ObservedTraitsDto = ObservedTraitsDto(),
)

data class ObservedTraitsDto(
    @SerializedName("band_color") val bandColor: String? = null,
)

data class ReconcileResponse(
    @SerializedName("results") val results: List<ReconcileResult> = emptyList(),
    /** Tags that resolved to no present group member. */
    @SerializedName("unmatched_tags") val unmatchedTags: List<String> = emptyList(),
)

data class ReconcileResult(
    @SerializedName("ref_id") val refId: String,
    @SerializedName("outcome") val outcome: ReconcileOutcome,
)

/** Flattened tagged union: `kind` selects which fields are populated.
 *  kind = "resolved"  → tagId + confidence
 *  kind = "ambiguous" → candidates (ranked best-first)
 *  kind = "no_candidate" → none */
data class ReconcileOutcome(
    @SerializedName("kind") val kind: String,
    @SerializedName("tag_id") val tagId: String? = null,
    /** "sole" | "forced" when kind = "resolved". */
    @SerializedName("confidence") val confidence: String? = null,
    @SerializedName("candidates") val candidates: List<ReconcileCandidate> = emptyList(),
)

data class ReconcileCandidate(
    @SerializedName("tag_id") val tagId: String,
    /** Soft-trait Jaccard similarity, 0.0–1.0. */
    @SerializedName("score") val score: Double,
)

/** Server snapshot powering the Flock-screen cull-mode guardrail. */
data class FlockBreedingStats(
    @SerializedName("total_males") val totalMales: Int,
    @SerializedName("total_females") val totalFemales: Int,
    @SerializedName("minimum_males_needed") val minimumMalesNeeded: Int,
    @SerializedName("safe_to_cull") val safeToCull: Int,
    @SerializedName("per_male_safe_pairings") val perMaleSafePairings: List<PerMaleSafePairings> = emptyList(),
    /** Echoed from /api/settings so the client can recompute the required-males
     *  line as the user toggles cull selections (e.g. selecting females
     *  reduces minimum_males_needed). */
    @SerializedName("desired_males_per_group") val desiredMalesPerGroup: Int = 1,
    @SerializedName("max_females_per_male") val maxFemalesPerMale: Int = 5,
)

data class PerMaleSafePairings(
    @SerializedName("bird_id") val birdId: Int,
    @SerializedName("safe_pairings") val safePairings: Int,
    @SerializedName("safe_female_ids") val safeFemaleIds: List<Int> = emptyList(),
)

data class AppSettings(
    @SerializedName("desired_males_per_group") val desiredMalesPerGroup: Int,
    @SerializedName("max_females_per_male") val maxFemalesPerMale: Int,
)

/** Partial-update payload — fields omitted (null) are left unchanged server-side. */
data class UpdateAppSettings(
    @SerializedName("desired_males_per_group") val desiredMalesPerGroup: Int? = null,
    @SerializedName("max_females_per_male") val maxFemalesPerMale: Int? = null,
)

/** Subset of GET /api/system-settings the app uses (the indoor-cam toggles).
 *  Other system-settings fields are present in the response but ignored here. */
data class SystemSettings(
    @SerializedName("indoor_cam_roboflow_upload_enabled") val indoorCamRoboflowUploadEnabled: Boolean = true,
    @SerializedName("indoor_cam_image_save_enabled") val indoorCamImageSaveEnabled: Boolean = true,
)

data class InbreedingCheckResult(
    @SerializedName("male_id") val maleId: Int,
    @SerializedName("female_id") val femaleId: Int,
    @SerializedName("coefficient") val coefficient: Double,
    @SerializedName("safe") val safe: Boolean,
    @SerializedName("warning") val warning: String? = null,
)

/** One male×female relatedness coefficient from `/api/breeding/suggest`.
 *  `coefficient` is on the standard ~0.5 scale (full sib / parent-offspring
 *  = 0.5, half-sib = 0.25, cousin-ish ≈ 0.125, unrelated = 0.0). */
data class InbreedingCoefficient(
    @SerializedName("male_id") val maleId: Int,
    @SerializedName("female_id") val femaleId: Int,
    @SerializedName("coefficient") val coefficient: Double,
    @SerializedName("safe") val safe: Boolean,
)

data class MortalityRequest(
    @SerializedName("count") val count: Int,
    @SerializedName("reason") val reason: String,
)

data class CullBatchRequest(
    @SerializedName("bird_ids") val birdIds: List<Int>,
    @SerializedName("reason") val reason: String,
    @SerializedName("method") val method: String,
    @SerializedName("notes") val notes: String? = null,
    @SerializedName("processed_date") val processedDate: String,
)

data class CullBatchResponse(
    @SerializedName("updated") val updated: Int,
)

// =====================================================================
// System alerts (backup/maintenance script failures, surfaced via the
// Dashboard bell icon). Distinct from BrooderAlert above.
// =====================================================================

data class SystemAlertDto(
    @SerializedName("id") val id: Long,
    @SerializedName("alert_key") val alertKey: String,
    @SerializedName("severity") val severity: String,
    @SerializedName("title") val title: String,
    @SerializedName("message") val message: String,
    @SerializedName("source") val source: String,
    @SerializedName("created_at") val createdAt: String,
    @SerializedName("resolved_at") val resolvedAt: String? = null,
    @SerializedName("dismissed_at") val dismissedAt: String? = null,
    @SerializedName("metadata_json") val metadataJson: String? = null,
    @SerializedName("is_active") val isActive: Boolean = true,
)

// =====================================================================
// Dev/test endpoint DTOs (only used when the server is built with
// DEV_MODE=true). See routes/dev.rs on the server side.
// =====================================================================

data class DevStatusResponse(
    @SerializedName("dev_mode") val devMode: Boolean,
    @SerializedName("has_backup") val hasBackup: Boolean,
)

data class DevSeedResponse(
    @SerializedName("status") val status: String,
    @SerializedName("backup") val backup: String,
)

data class DevRestoreResponse(
    @SerializedName("status") val status: String,
)

@Suppress("unused")
interface QuailSyncApi {

    @GET("api/brooders")
    suspend fun getBrooders(): List<Brooder>

    /** Create a new housing unit. Body keys: name, lineage_id, life_stage,
     *  qr_code, notes, camera_url, housing_type. Server defaults housing_type
     *  to "brooder" when omitted. */
    @POST("api/brooders")
    suspend fun createBrooder(@Body body: Map<String, @JvmSuppressWildcards Any?>): Brooder

    /** Issue #13: assign a batch of birds to a housing unit. Body:
     *  `{"bird_ids":[…]}`. Server validates all ids before any writes. */
    @POST("api/brooders/{id}/assign-birds")
    suspend fun assignBirdsToHousing(
        @Path("id") housingId: Int,
        @Body body: BirdAssignmentRequest,
    ): BirdAssignmentResponse

    /** Issue #13: clear housing assignment for a batch of birds currently
     *  housed in `id`. Tolerant — unhoused birds and birds elsewhere are no-ops. */
    @POST("api/brooders/{id}/unassign-birds")
    suspend fun unassignBirdsFromHousing(
        @Path("id") housingId: Int,
        @Body body: BirdAssignmentRequest,
    ): BirdAssignmentResponse

    /** Issue #14: move an already-graduated chick group (and every bird it
     *  produced) into this hutch. Server validates housing type and group
     *  status (must be `Graduated`). */
    @POST("api/brooders/{id}/assign-graduated-group")
    suspend fun assignGraduatedGroupToHousing(
        @Path("id") housingId: Int,
        @Body body: AssignGraduatedGroupRequest,
    ): AssignGraduatedGroupResponse

    @GET("api/brooders/{id}/readings")
    suspend fun getBrooderReadings(@Path("id") id: Int): List<BrooderReading>

    @GET("api/brooders/{id}/alerts")
    suspend fun getBrooderAlerts(@Path("id") id: Int): List<BrooderAlert>

    @GET("api/birds")
    suspend fun getBirds(): List<Bird>

    @POST("api/birds")
    suspend fun createBird(@Body request: CreateBirdRequest): Bird

    @PUT("api/birds/{id}")
    suspend fun updateBird(@Path("id") id: Int, @Body request: UpdateBirdRequest): Bird

    @DELETE("api/birds/{id}")
    suspend fun deleteBird(@Path("id") id: Int): retrofit2.Response<Unit>

    @GET("api/birds/{id}/weights")
    suspend fun getBirdWeights(@Path("id") id: Int): List<BirdWeight>

    @POST("api/birds/{id}/weight")
    suspend fun createBirdWeight(@Path("id") id: Int, @Body request: CreateWeightRequest): BirdWeight

    @DELETE("api/birds/{id}/weights/{wid}")
    suspend fun deleteBirdWeight(@Path("id") birdId: Int, @Path("wid") weightId: Int): retrofit2.Response<Unit>

    @GET("api/birds/nfc/{tag_id}")
    suspend fun getBirdByNfcTag(@Path("tag_id") tagId: String): Bird

    @Multipart
    @POST("api/birds/{id}/photo")
    suspend fun uploadBirdPhoto(
        @Path("id") id: Int,
        @Part photo: MultipartBody.Part,
    ): PhotoUploadResponse

    @GET("api/birds/{id}/photos")
    suspend fun getBirdPhotos(@Path("id") id: Int): List<BirdPhoto>

    @GET("api/trailcam/cameras")
    suspend fun getTrailcamCameras(): List<TrailcamCamera>

    @GET("api/trailcam/latest/{camera_id}")
    suspend fun getTrailcamLatest(@Path("camera_id") cameraId: String): TrailcamLatest

    /** Every observation for a camera within the last [hours] (default 168 =
     *  7 days), returned oldest-first. Items share [TrailcamLatest]'s shape. */
    @GET("api/trailcam/history/{camera_id}")
    suspend fun getTrailcamHistory(
        @Path("camera_id") cameraId: String,
        @retrofit2.http.Query("hours") hours: Int = 168,
    ): List<TrailcamLatest>

    // Govee sensors: list all (with assignment + latest reading), assign, unassign.
    @GET("api/govee/sensors")
    suspend fun getGoveeSensors(): List<GoveeSensorDto>

    @PUT("api/govee/sensors/{id}/assign")
    suspend fun assignGoveeSensor(
        @Path("id") id: Int,
        @Body body: AssignSensorRequest,
    ): GoveeSensorDto

    @retrofit2.http.HTTP(method = "DELETE", path = "api/govee/sensors/{id}/assign", hasBody = false)
    suspend fun unassignGoveeSensor(@Path("id") id: Int): retrofit2.Response<Unit>

    // SPYPOINT trail cameras: list all (with assignment), assign, unassign.
    @GET("api/trail-cameras")
    suspend fun getTrailCameras(): List<TrailCameraDto>

    @PUT("api/trail-cameras/{id}/assign")
    suspend fun assignTrailCamera(
        @Path("id") id: Int,
        @Body body: AssignCameraRequest,
    ): TrailCameraDto

    @retrofit2.http.HTTP(method = "DELETE", path = "api/trail-cameras/{id}/assign", hasBody = false)
    suspend fun unassignTrailCamera(@Path("id") id: Int): retrofit2.Response<Unit>

    // Indoor cameras: observation read (count + image) + registry/assignment.
    @GET("api/indoorcam/cameras")
    suspend fun getIndoorcamCameras(): List<TrailcamCamera>

    @GET("api/indoorcam/latest/{camera_id}")
    suspend fun getIndoorcamLatest(@Path("camera_id") cameraId: String): IndoorcamLatest

    @GET("api/indoor-cameras")
    suspend fun getIndoorCameras(): List<IndoorCamera>

    @PUT("api/indoor-cameras/{id}/assign")
    suspend fun assignIndoorCamera(
        @Path("id") id: Int,
        @Body body: AssignIndoorCameraRequest,
    ): IndoorCamera

    @retrofit2.http.HTTP(method = "DELETE", path = "api/indoor-cameras/{id}/assign", hasBody = false)
    suspend fun unassignIndoorCamera(@Path("id") id: Int): retrofit2.Response<Unit>

    @GET("api/lineages")
    suspend fun getLineages(): List<Lineage>

    @POST("api/lineages")
    suspend fun createLineage(@Body request: CreateLineageRequest): Lineage

    @GET("api/clutches")
    suspend fun getClutches(): List<Clutch>

    @POST("api/clutches")
    suspend fun createClutch(@Body request: CreateClutchRequest): Clutch

    @PUT("api/clutches/{id}")
    suspend fun updateClutch(@Path("id") id: Int, @Body request: UpdateClutchRequest): Clutch

    @DELETE("api/clutches/{id}")
    suspend fun deleteClutch(@Path("id") id: Int): retrofit2.Response<Unit>

    @POST("api/chick-groups")
    suspend fun createChickGroup(@Body request: CreateChickGroupRequest): ChickGroupDto

    /** Generic chick-group update — backend accepts a JSON body with any of
     *  current_count / brooder_id / notes / status. Used after a batch
     *  finishes graduating to flip status='Graduated'. */
    @PUT("api/chick-groups/{id}")
    suspend fun updateChickGroup(
        @Path("id") id: Int,
        @Body body: Map<String, @JvmSuppressWildcards Any?>,
    ): retrofit2.Response<Unit>

    @DELETE("api/chick-groups/{id}")
    suspend fun deleteChickGroup(@Path("id") id: Int): retrofit2.Response<Unit>

    @GET("api/cameras")
    suspend fun getCameras(): List<Camera>

    @POST("api/cameras")
    suspend fun createCamera(@Body request: CreateCameraRequest): Camera

    @DELETE("api/cameras/{id}")
    suspend fun deleteCamera(@Path("id") id: Int): retrofit2.Response<Unit>

    @PUT("api/brooders/{id}")
    suspend fun updateBrooder(@Path("id") id: Int, @Body request: UpdateBrooderRequest): Brooder

    @GET("api/brooders/{id}/target-temp")
    suspend fun getBrooderTargetTemp(@Path("id") id: Int): TargetTempResponse

    @GET("api/brooders/{id}/headcount/latest")
    suspend fun getHeadcountLatest(@Path("id") id: Int): HeadcountResponse

    @PUT("api/brooders/{id}/assign-group")
    suspend fun assignGroupToBrooder(@Path("id") id: Int, @Body request: AssignGroupRequest): ChickGroupDto

    @GET("api/brooders/{id}/residents")
    suspend fun getBrooderResidents(@Path("id") id: Int): BrooderResidentsResponse

    @GET("api/chick-groups")
    suspend fun getChickGroups(): List<ChickGroupDto>

    @POST("api/chick-groups/{id}/mortality")
    suspend fun logMortality(@Path("id") groupId: Int, @Body request: MortalityRequest): ChickGroupDto

    /** Replace the chick group's lineage set atomically. Body must contain ≥1 ID. */
    @PUT("api/chick-groups/{id}/lineages")
    suspend fun replaceChickGroupLineages(
        @Path("id") groupId: Int,
        @Body request: ReplaceLineagesRequest,
    ): ChickGroupDto

    /** Replace a bird's lineage set atomically. Body must contain ≥1 ID. */
    @PUT("api/birds/{id}/lineages")
    suspend fun replaceBirdLineages(
        @Path("id") birdId: Int,
        @Body request: ReplaceLineagesRequest,
    ): Bird

    @PUT("api/birds/{id}/move")
    suspend fun moveBird(@Path("id") id: Int, @Body request: MoveBirdRequest): Bird

    // Delete brooder
    @retrofit2.http.HTTP(method = "DELETE", path = "api/brooders/{id}", hasBody = false)
    suspend fun deleteBrooder(@Path("id") id: Int): retrofit2.Response<Unit>

    // Breeding groups
    @GET("api/breeding-groups")
    suspend fun getBreedingGroups(): List<BreedingGroupDto>

    @POST("api/breeding-groups")
    suspend fun createBreedingGroup(@Body request: CreateBreedingGroupRequest): BreedingGroupDto

    @PUT("api/breeding-groups/{id}")
    suspend fun updateBreedingGroup(
        @Path("id") id: Int,
        @Body request: UpdateBreedingGroupRequest,
    ): BreedingGroupDto

    @DELETE("api/breeding-groups/{id}")
    suspend fun deleteBreedingGroup(@Path("id") id: Int): retrofit2.Response<Unit>

    /** Read-only deduction: which present unbanded bird does each dropped tag
     *  belong to? Never writes a band assignment. */
    @POST("api/groups/{id}/reconcile-tags")
    suspend fun reconcileTags(
        @Path("id") groupId: Int,
        @Body request: ReconcileRequest,
    ): ReconcileResponse

    // Flock breeding stats (powers the cull-mode guardrail UI).
    // Same path as before — server response shape changed from a prescribed
    // cull list to a stats snapshot. Old field name kept on the path for
    // backwards compatibility with the dashboard's URL.
    @GET("api/flock/cull-recommendations")
    suspend fun getFlockBreedingStats(): FlockBreedingStats

    /** All active male×female relatedness coefficients, reusing the server's
     *  `compute_relatedness`. Fetched once to power the create-group inbreeding
     *  warning without a round-trip per pair. */
    @GET("api/breeding/suggest")
    suspend fun getBreedingSuggestions(): List<InbreedingCoefficient>

    // App settings (breeding ratio config).
    @GET("api/settings")
    suspend fun getSettings(): AppSettings

    @PUT("api/settings")
    suspend fun updateSettings(@Body body: UpdateAppSettings): AppSettings

    // System settings (server-owned). The app only surfaces the indoor-cam
    // image toggles; PUT is a partial update keyed by setting name.
    @GET("api/system-settings")
    suspend fun getSystemSettings(): SystemSettings

    @PUT("api/system-settings")
    suspend fun updateSystemSettings(
        @Body body: Map<String, @JvmSuppressWildcards Any?>,
    ): SystemSettings

    // Inbreeding check
    @GET("api/inbreeding-check")
    suspend fun checkInbreeding(
        @retrofit2.http.Query("male_id") maleId: Int,
        @retrofit2.http.Query("female_id") femaleId: Int,
    ): InbreedingCheckResult

    // Batch cull
    @POST("api/cull-batch")
    suspend fun cullBatch(@Body request: CullBatchRequest): CullBatchResponse

    // System alerts (Pi maintenance scripts)
    @GET("api/alerts/active")
    suspend fun getActiveAlerts(): List<SystemAlertDto>

    @GET("api/alerts/recent")
    suspend fun getRecentAlerts(@retrofit2.http.Query("limit") limit: Int = 50): List<SystemAlertDto>

    @POST("api/alerts/{id}/dismiss")
    suspend fun dismissAlert(@Path("id") id: Long): SystemAlertDto

    // ---------------------------------------------------------------------
    // Dev/test endpoints — only exist server-side when DEV_MODE=true. The
    // status endpoint is the discovery probe: a 404 means dev mode is off
    // (or we're talking to a prod build), so callers use Response<T> and
    // treat null/non-2xx as "hide the dev UI".
    // ---------------------------------------------------------------------

    @GET("api/dev/status")
    suspend fun getDevStatus(): retrofit2.Response<DevStatusResponse>

    @POST("api/dev/seed")
    suspend fun seedDevData(): retrofit2.Response<DevSeedResponse>

    @POST("api/dev/stress-seed")
    suspend fun stressSeedDevData(): retrofit2.Response<DevSeedResponse>

    @POST("api/dev/restore")
    suspend fun restoreDevData(): retrofit2.Response<DevRestoreResponse>

    companion object {
        fun create(baseUrl: String): QuailSyncApi {
            val url = if (baseUrl.endsWith("/")) baseUrl else "$baseUrl/"
            val logging = HttpLoggingInterceptor { message ->
                android.util.Log.d("QuailSync-HTTP", message)
            }.apply {
                level = HttpLoggingInterceptor.Level.BODY
            }
            val client = OkHttpClient.Builder()
                .addInterceptor(logging)
                .build()
            return Retrofit.Builder()
                .baseUrl(url)
                .client(client)
                .addConverterFactory(GsonConverterFactory.create())
                .build()
                .create(QuailSyncApi::class.java)
        }
    }
}
