package dev.mihomo.vpn

import android.content.Context
import android.content.pm.ApplicationInfo
import android.graphics.Bitmap
import android.graphics.Canvas
import android.util.Base64
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors

internal object InstalledApps {
    // Package enumeration and icons must not wait behind core startup or validation.
    val executor = Executors.newFixedThreadPool(2)
    private var cached: List<Map<String, Any?>>? = null
    private var expires = 0L

    @Synchronized fun list(context: Context): List<Map<String, Any?>> {
        val now = android.os.SystemClock.elapsedRealtime()
        cached?.let { if (now < expires) return it }
        val pm = context.packageManager
        val result = pm.getInstalledApplications(0)
            .filter { it.packageName != context.packageName && it.packageName != "android" }
            .map { app -> mapOf<String, Any?>(
                "pid" to 0, "name" to app.loadLabel(pm).toString(), "path" to null,
                "packageName" to app.packageName, "system" to (app.flags and ApplicationInfo.FLAG_SYSTEM != 0), "icon" to null
            ) }.sortedBy { it["name"].toString().lowercase() }
        cached = result
        expires = now + 30_000
        return result
    }
    fun icon(context: Context, packageName: String): String? = runCatching {
        val drawable = context.packageManager.getApplicationIcon(packageName)
        val bitmap = Bitmap.createBitmap(48, 48, Bitmap.Config.ARGB_8888)
        try {
            drawable.setBounds(0, 0, 48, 48)
            drawable.draw(Canvas(bitmap))
            val output = ByteArrayOutputStream()
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)
            "data:image/png;base64," + Base64.encodeToString(output.toByteArray(), Base64.NO_WRAP)
        } finally { bitmap.recycle() }
    }.getOrNull()
}
