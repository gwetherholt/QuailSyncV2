package com.quailsync.app.data

import com.google.gson.annotations.SerializedName
import com.quailsync.app.BuildConfig
import retrofit2.Retrofit
import retrofit2.converter.gson.GsonConverterFactory
import retrofit2.http.GET
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
    @SerializedName("latest_temperature_celsius") val latestTemperatureCelsius: Double? = null,
    @SerializedName("latest_humidity_percent") val latestHumidityPercent: Double? = null,
)

data class BrooderReading(
    @SerializedName("id") val id: Int? = null,
    @SerializedName("brooder_id") val brooderId: Int,
    @SerializedName("temperature_celsius") val temperature: Double?,
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
    @SerializedName("recorded_at") val recordedAt: String? = null,
)

data class Bloodline(
    @SerializedName("id") val id: Int,
    @SerializedName("name") val name: String,
    @SerializedName("color") val color: String? = null,
    @SerializedName("description") val description: String? = null,
)

data class Clutch(
    @SerializedName("id") val id: Int,
    @SerializedName("parent_pair_id") val parentPairId: Int? = null,
    @SerializedName("bloodline_id") val bloodlineId: Int? = null,
    @SerializedName("bloodline_name") val bloodlineName: String? = null,
    @SerializedName("egg_count") val eggCount: Int? = null,
    @SerializedName("fertile_count") val fertileCount: Int? = null,
    @SerializedName("hatch_count") val hatchCount: Int? = null,
    @SerializedName("set_date") val setDate: String? = null,
    @SerializedName("expected_hatch_date") val expectedHatchDate: String? = null,
    @SerializedName("status") val status: String? = null,
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

interface QuailSyncApi {

    @GET("api/brooders")
    suspend fun getBrooders(): List<Brooder>

    @GET("api/brooders/{id}/readings")
    suspend fun getBrooderReadings(@Path("id") id: Int): List<BrooderReading>

    @GET("api/brooders/{id}/alerts")
    suspend fun getBrooderAlerts(@Path("id") id: Int): List<BrooderAlert>

    @GET("api/birds")
    suspend fun getBirds(): List<Bird>

    @GET("api/birds/{id}/weights")
    suspend fun getBirdWeights(@Path("id") id: Int): List<BirdWeight>

    @GET("api/bloodlines")
    suspend fun getBloodlines(): List<Bloodline>

    @GET("api/clutches")
    suspend fun getClutches(): List<Clutch>

    @GET("api/cameras")
    suspend fun getCameras(): List<Camera>

    companion object {
        fun create(baseUrl: String = BuildConfig.BASE_URL): QuailSyncApi {
            val url = if (baseUrl.endsWith("/")) baseUrl else "$baseUrl/"
            return Retrofit.Builder()
                .baseUrl(url)
                .addConverterFactory(GsonConverterFactory.create())
                .build()
                .create(QuailSyncApi::class.java)
        }
    }
}
