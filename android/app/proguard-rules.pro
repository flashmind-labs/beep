# JNA looks up classes, fields and callbacks by reflection.
-keep class com.sun.jna.** { *; }
-keep class * extends com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-dontwarn java.awt.**

# uniffi bindings are called through JNA by name.
-keep class com.flashmind.beep.ffi.** { *; }
-keep class com.flashmind.beep.proto.** { *; }

# JLatexMath looks up its macros and alphabets by class name.
-keep class org.scilab.forge.jlatexmath.** { *; }
