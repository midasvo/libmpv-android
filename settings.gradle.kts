include(":libmpv")
// Not published: an app that uses the library minified by R8, for the consumer rules' test
include(":r8-test")

pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
        google()
    }
}