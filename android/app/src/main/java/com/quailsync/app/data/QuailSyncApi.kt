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
    @SerializedName("bloodline_id") val bloodlineId: Int? = null,
    @SerializedName("life_stage") val lifeStage: String? = null,
)

data class UpdateBrooderRequest(
    @SerializedName("camera_url") val cameraUrl: String? = null,
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
    @SerializedName("bloodline_id") val bloodlineId: Int? = null,
    @SerializedName("bloodline_name") val bloodlineName: String? = null,
    @SerializedName("brooder_id") val brooderId: Int? = null,
    @SerializedName("notes") val notes: String? = null,
    @SerializedName("sire_id") val sireId: Int? = null,
    @SerializedName("dam_id") val damId: Int? = null,
    @SerializedName("latest_weight") val latestWeight: Double? = null,
)

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

data class Bloodline(
    @SerializedName("id") val id: Int,
    @SerializedName("name") val name: String,
    @SerializedName("source") val source: String? = null,
    @SerializedName("notes") val notes: String? = null,
)

data class CreateBloodlineRequest(
    @SerializedName("name") val name: String,
    @SerializedName("source") val source: String = "",
    @SerializedName("notes") val notes: String? = null,
)

data class Clutch(
    @SerializedName("id") val id: Int,
    @SerializedName("parent_pair_id") val parentPairId: Int? = null,
    @SerializedName("bloodline_id") val bloodlineId: Int? = null,
    @SerializedName("bloodline_name") val bloodlineName: String? = null,
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
) {
    /** Eggs set — prefers eggs_set (server field) over egg_count (legacy). */
    val totalEggs: Int? get() = eggsSet ?: eggCount
    /** Fertile eggs — prefers eggs_fertile (server field) over fertile_count (legacy). */
    val totalFertile: Int? get() = eggsFertile ?: fertileCount
    /** Hatched eggs — prefers eggs_hatched (server field) over hatch_count (legacy). */
    val totalHatched: Int? get() = eggsHatched ?: hatchCount
}

data class CreateClutchRequest(
    @SerializedName("bloodline_id") val bloodlineId: Int? = null,
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
    @SerializedName("bloodline_id") val bloodlineId: Int,
    @SerializedName("brooder_id") val brooderId: Int? = null,
    @SerializedName("initial_count") val initialCount: Int,
    @SerializedName("hatch_date") val hatchDate: String,
    @SerializedName("notes") val notes: String? = null,
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
)

data class CreateBirdRequest(
    @SerializedName("band_color") val bandColor: String? = null,
    @SerializedName("sex") val sex: String = "Unknown",
    @SerializedName("bloodline_id") val bloodlineId: Long,
    @SerializedName("hatch_date") val hatchDate: String,
    @SerializedName("mother_id") val motherId: Long? = null,
    @SerializedName("father_id") val fatherId: Long? = null,
    @SerializedName("generation") val generation: Int = 1,
    @SerializedName("status") val status: String = "Active",
    @SerializedName("notes") val notes: String? = null,
    @SerializedName("nfc_tag_id") val nfcTagId: String? = null,
)

data class PhotoUploadResponse(
    @SerializedName("id") val id: Int? = null,
    @SerializedName("url") val url: String? = null,
    @SerializedName("path") val path: String? = null,
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

data class ChickGroupDto(
    @SerializedName("id") val id: Int,
    @SerializedName("clutch_id") val clutchId: Int? = null,
    @SerializedName("bloodline_id") val bloodlineId: Int,
    @SerializedName("brooder_id") val brooderId: Int? = null,
    @SerializedName("initial_count") val initialCount: Int,
    @SerializedName("current_count") val currentCount: Int,
    @SerializedName("hatch_date") val hatchDate: String,
    @SerializedName("status") val status: String,
    @SerializedName("notes") val notes: String? = null,
)

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
    @SerializedName("male_id") val maleId: Int,
    @SerializedName("female_ids") val femaleIds: List<Int> = emptyList(),
    @SerializedName("start_date") val startDate: String? = null,
    @SerializedName("notes") val notes: String? = null,
)

data class CreateBreedingGroupRequest(
    @SerializedName("name") val name: String,
    @SerializedName("male_id") val maleId: Int,
    @SerializedName("female_ids") val femaleIds: List<Int>,
    @SerializedName("start_date") val startDate: String,
    @SerializedName("notes") val notes: String? = null,
)

data class CullRecommendation(
    @SerializedName("bird_id") val birdId: Int,
    @SerializedName("reason") val reason: com.google.gson.JsonElement? = null,
) {
    /** Parse the Rust serde-tagged enum: "ExcessMale" or {"LowWeight": {"weight_grams": N}} */
    val reasonLabel: String get() {
        val r = reason ?: return "Unknown"
        if (r.isJsonPrimitive) return when (r.asString) {
            "ExcessMale" -> "Excess Male"
            else -> r.asString
        }
        if (r.isJsonObject) {
            val obj = r.asJsonObject
            if (obj.has("LowWeight")) {
                val w = obj.getAsJsonObject("LowWeight").get("weight_grams")?.asDouble ?: 0.0
                return "Underweight (${w.toInt()}g)"
            }
            if (obj.has("HighInbreeding")) {
                val c = obj.getAsJsonObject("HighInbreeding").get("coefficient")?.asDouble ?: 0.0
                return "Inbreeding Risk (${"%.0f".format(c * 100)}%)"
            }
        }
        return "Unknown"
    }
    val reasonKey: String get() {
        val r = reason ?: return "unknown"
        if (r.isJsonPrimitive && r.asString == "ExcessMale") return "excess_male"
        if (r.isJsonObject) {
            val obj = r.asJsonObject
            if (obj.has("LowWeight")) return "underweight"
            if (obj.has("HighInbreeding")) return "inbreeding"
        }
        return "unknown"
    }
    val priority: String get() = when (reasonKey) {
        "inbreeding" -> "high"
        "excess_male" -> "medium"
        "underweight" -> "low"
        else -> "low"
    }
}

data class InbreedingCheckResult(
    @SerializedName("male_id") val maleId: Int,
    @SerializedName("female_id") val femaleId: Int,
    @SerializedName("coefficient") val coefficient: Double,
    @SerializedName("safe") val safe: Boolean,
    @SerializedName("warning") val warning: String? = null,
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

interface QuailSyncApi {

    @GET("api/brooders")
    suspend fun getBrooders(): List<Brooder>

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

    @GET("api/bloodlines")
    suspend fun getBloodlines(): List<Bloodline>

    @POST("api/bloodlines")
    suspend fun createBloodline(@Body request: CreateBloodlineRequest): Bloodline

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

    @PUT("api/brooders/{id}/assign-group")
    suspend fun assignGroupToBrooder(@Path("id") id: Int, @Body request: AssignGroupRequest): ChickGroupDto

    @GET("api/brooders/{id}/residents")
    suspend fun getBrooderResidents(@Path("id") id: Int): BrooderResidentsResponse

    @GET("api/chick-groups")
    suspend fun getChickGroups(): List<ChickGroupDto>

    @PUT("api/birds/{id}/move")
    suspend fun moveBird(@Path("id") id: Int, @Body request: MoveBirdRequest): Bird

    // Breeding groups
    @GET("api/breeding-groups")
    suspend fun getBreedingGroups(): List<BreedingGroupDto>

    @POST("api/breeding-groups")
    suspend fun createBreedingGroup(@Body request: CreateBreedingGroupRequest): BreedingGroupDto

    // Cull recommendations
    @GET("api/flock/cull-recommendations")
    suspend fun getCullRecommendations(): List<CullRecommendation>

    // Inbreeding check
    @GET("api/inbreeding-check")
    suspend fun checkInbreeding(
        @retrofit2.http.Query("male_id") maleId: Int,
        @retrofit2.http.Query("female_id") femaleId: Int,
    ): InbreedingCheckResult

    // Batch cull
    @POST("api/cull-batch")
    suspend fun cullBatch(@Body request: CullBatchRequest): CullBatchResponse

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
