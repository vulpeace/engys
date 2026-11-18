package dev.mihomo.vpn

import java.io.File

internal fun residentMemory(): Long? = runCatching {
    File("/proc/self/status").useLines { lines ->
        lines.firstOrNull { it.startsWith("VmRSS:") }
            ?.substringAfter(':')?.trim()?.split(Regex("\\s+"))?.firstOrNull()?.toLongOrNull()?.times(1024)
    }
}.getOrNull()
