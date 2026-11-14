# The QR scanner's ML Kit reads its own internals by reflection; shrunk, it
# crashes when the camera opens.
-keep class com.google.mlkit.** { *; }
-keep class com.google.android.gms.internal.mlkit_** { *; }
-keep class com.google.android.odml.** { *; }
