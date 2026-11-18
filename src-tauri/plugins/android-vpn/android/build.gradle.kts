import org.jetbrains.kotlin.gradle.dsl.JvmTarget
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}
android {
    namespace = "dev.mihomo.vpn"
    compileSdk = 36
    ndkVersion = "28.2.13676358"
    defaultConfig { minSdk = 29; consumerProguardFiles("consumer-rules.pro") }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("core/jni"))
}
kotlin { compilerOptions { jvmTarget = JvmTarget.JVM_1_8 } }
val coreRoot = rootProject.file("../../../core-mobile")
val coreOutput = layout.buildDirectory.dir("core")
val buildCore = tasks.register<Exec>("buildMobileCore") {
    inputs.files(fileTree(coreRoot) { include("**/*.go", "go.mod", "go.sum") })
    inputs.file(coreRoot.resolve("build-android.sh"))
    val requestedAbis = providers.gradleProperty("abiList").orElse("arm64-v8a,x86_64")
    val targets = providers.gradleProperty("mihomo.core.targets").orElse(requestedAbis.map { abis ->
        abis.split(',').joinToString(",") { abi -> when (abi.trim()) {
            "arm64-v8a" -> "android/arm64"
            "x86_64" -> "android/amd64"
            else -> throw GradleException("Unsupported Android ABI: $abi. Build aarch64 or x86_64.")
        } }
    })
    inputs.property("targets", targets)
    outputs.dir(coreOutput)
    workingDir(coreRoot)
    environment("ANDROID_HOME", android.sdkDirectory.absolutePath)
    environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    commandLine("bash", coreRoot.resolve("build-android.sh").absolutePath, coreOutput.get().asFile.absolutePath, targets.get())
}
tasks.named("preBuild").configure { dependsOn(buildCore) }
// Generated bindings and native libraries come from the same pinned gomobile build.
dependencies {
    implementation(project(":tauri-android"))
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation(files(coreOutput.map { it.file("classes.jar") }).builtBy(buildCore))
    testImplementation("junit:junit:4.13.2")
}
