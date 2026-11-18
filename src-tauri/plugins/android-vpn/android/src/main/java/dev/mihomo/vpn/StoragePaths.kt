package dev.mihomo.vpn

import java.io.File

internal fun privateChild(root: File, candidate: File): File {
    val directory = root.canonicalFile
    val child = candidate.canonicalFile
    require(child != directory && child.toPath().startsWith(directory.toPath())) {
        "Core storage must be inside the application's private directory"
    }
    return child
}
