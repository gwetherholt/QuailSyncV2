package com.quailsync.app.data

import com.quailsync.app.BuildConfig
import retrofit2.Retrofit
import retrofit2.converter.gson.GsonConverterFactory
import retrofit2.http.GET
import retrofit2.http.Path

data class Brooder(
    val id: Int,
    val name: String,
    val location: String? = null,
    val capacity: Int? = null,
    val status: String? = null,
)

data class BrooderReading(
    val id: Int? = null,
    val brooder_id: Int,
    val temperature: Double?,
    val humidity: Double?,
    val recorded_at: String? = null,
)

data class BrooderAlert(
    val id: Int? = null,
    val brooder_id: Int,
    val alert_type: String,
    val severity: String? = null,
    val message: String? = null,
    val acknowledged: Boolean? = false,
    val created_at: String? = null,
)

data class Bird(
    val id: Int,
    val band_id: String? = null,
    val species: String? = null,
    val sex: String? = null,
    val status: String? = null,
    val hatch_date: String? = null,
    val bloodline_id: Int? = null,
    val brooder_id: Int? = null,
    val notes: String? = null,
)

data class Bloodline(
    val id: Int,
    val name: String,
    val color: String? = null,
    val description: String? = null,
)

data class Clutch(
    val id: Int,
    val parent_pair_id: Int? = null,
    val egg_count: Int? = null,
    val fertile_count: Int? = null,
    val hatch_count: Int? = null,
    val set_date: String? = null,
    val expected_hatch_date: String? = null,
    val status: String? = null,
)

data class Camera(
    val id: Int,
    val name: String,
    val url: String? = null,
    val location: String? = null,
    val status: String? = null,
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
