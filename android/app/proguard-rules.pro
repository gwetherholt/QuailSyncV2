# Retrofit
-keepattributes Signature
-keepattributes *Annotation*
-keep class com.quailsync.app.data.** { *; }
-keepclassmembers,allowshrinking,allowobfuscation interface * {
    @retrofit2.http.* <methods>;
}

# Gson
-keep class com.google.gson.** { *; }
