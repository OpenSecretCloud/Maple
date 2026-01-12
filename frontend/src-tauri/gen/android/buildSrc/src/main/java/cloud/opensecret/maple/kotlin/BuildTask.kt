import java.io.File
import org.apache.tools.ant.taskdefs.condition.Os
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction

open class BuildTask : DefaultTask() {
    @Input
    var rootDirRel: String? = null
    @Input
    var target: String? = null
    @Input
    var release: Boolean? = null

    @TaskAction
    fun assemble() {
        if (shouldSkipRustBuild()) {
            val target = target ?: throw GradleException("target cannot be null")
            val lib = expectedJniLib(target)
            if (!lib.exists()) {
                throw GradleException(
                    "skipRustBuild=true but prebuilt Rust library is missing at ${lib.absolutePath}"
                )
            }

            project.logger.lifecycle("skipRustBuild=true; using prebuilt Rust library at ${lib.absolutePath}")
            return
        }

        val userHome = System.getProperty("user.home")
        val executable = if (File("$userHome/.bun/bin/bun").exists()) {
            "$userHome/.bun/bin/bun"
        } else {
            """bun"""
        }
        try {
            runTauriCli(executable)
        } catch (e: Exception) {
            if (Os.isFamily(Os.FAMILY_WINDOWS)) {
                runTauriCli("$executable.cmd")
            } else {
                throw e;
            }
        }
    }

    private fun shouldSkipRustBuild(): Boolean {
        val raw = project.findProperty("skipRustBuild")?.toString() ?: return false
        return raw.equals("true", ignoreCase = true) || raw == "1" || raw.equals("yes", ignoreCase = true)
    }

    private fun expectedJniLib(target: String): File {
        val abi = when (target) {
            "aarch64" -> "arm64-v8a"
            "armv7" -> "armeabi-v7a"
            "i686" -> "x86"
            "x86_64" -> "x86_64"
            else -> throw GradleException("Unknown target '$target'")
        }

        return File(project.projectDir, "src/main/jniLibs/$abi/libapp_lib.so")
    }

    fun runTauriCli(executable: String) {
        val rootDirRel = rootDirRel ?: throw GradleException("rootDirRel cannot be null")
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")
        val args = listOf("tauri", "android", "android-studio-script");

        project.exec {
            workingDir(File(project.projectDir, rootDirRel))
            executable(executable)
            args(args)
            if (project.logger.isEnabled(LogLevel.DEBUG)) {
                args("-vv")
            } else if (project.logger.isEnabled(LogLevel.INFO)) {
                args("-v")
            }
            if (release) {
                args("--release")
            }
            args(listOf("--target", target))
        }.assertNormalExitValue()
    }
}