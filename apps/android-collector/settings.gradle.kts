pluginManagement {
    repositories {
        // USTC 镜像优先，失败时回退官方源
        maven { url = uri("https://mirrors.ustc.edu.cn/google-android/") }
        maven { url = uri("https://mirrors.ustc.edu.cn/gradle/") }
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        // USTC 镜像优先，失败时回退官方源
        maven { url = uri("https://mirrors.ustc.edu.cn/google-android/") }
        maven { url = uri("https://mirrors.ustc.edu.cn/maven/") }
        google()
        mavenCentral()
    }
}

rootProject.name = "DiPECSAndroidCollector"
include(":app")
