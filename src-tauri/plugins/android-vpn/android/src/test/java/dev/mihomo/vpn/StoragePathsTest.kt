package dev.mihomo.vpn

import org.junit.Test
import org.junit.Assert.assertEquals
import java.nio.file.Files

class StoragePathsTest {
    @Test fun acceptsCanonicalAliases() {
        val temporary = Files.createTempDirectory("mihomo-paths")
        try {
            val root = Files.createDirectory(temporary.resolve("private"))
            val alias = Files.createSymbolicLink(temporary.resolve("alias"), root)
            val core = Files.createDirectory(root.resolve("core"))
            assertEquals(core.toFile(), privateChild(alias.toFile(), core.toFile()))
            assertEquals(core.toFile(), privateChild(root.toFile(), alias.resolve("core").toFile()))
        } finally { temporary.toFile().deleteRecursively() }
    }
    @Test(expected = IllegalArgumentException::class)
    fun rejectsSimilarPrefix() { privateChild(java.io.File("/tmp/app"), java.io.File("/tmp/app-other/core")) }
    @Test(expected = IllegalArgumentException::class)
    fun rejectsTraversal() { privateChild(java.io.File("/tmp/app"), java.io.File("/tmp/app/../outside")) }
}
