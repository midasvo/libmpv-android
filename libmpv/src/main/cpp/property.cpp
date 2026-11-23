#include <jni.h>
#include <cstdlib>

#include <mpv/client.h>

#include "jni_utils.h"
#include "log.h"
#include "globals.h"

extern "C" {
    jni_func(jint, nativeSetOptionString, jlong instance, jstring joption, jstring jvalue);

    jni_func(jobject, nativeGetPropertyInt, jlong instance, jstring jproperty);
    jni_func(jobject, nativeGetPropertyLong, jlong instance, jstring jproperty);
    jni_func(void, nativeSetPropertyInt, jlong instance, jstring jproperty, jint jvalue);
    jni_func(jobject, nativeGetPropertyDouble, jlong instance, jstring jproperty);
    jni_func(void, nativeSetPropertyDouble, jlong instance, jstring jproperty, jdouble jvalue);
    jni_func(jobject, nativeGetPropertyBoolean, jlong instance, jstring jproperty);
    jni_func(void, nativeSetPropertyBoolean, jlong instance, jstring jproperty, jboolean jvalue);
    jni_func(jstring, nativeGetPropertyString, jlong instance, jstring jproperty);
    jni_func(void, nativeSetPropertyString, jlong instance, jstring jproperty, jstring jvalue);

    jni_func(void, nativeObserveProperty, jlong instance, jstring jproperty, jint jformat);
}

jni_func(jint, nativeSetOptionString, jlong instance, jstring joption, jstring jvalue) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    if (!mpv_instance->mpv) {
        die(env, "mpv is not initialized");
        return 0;
    }

    ScopedUtfChars option(env, joption);
    if (!option.c_str())
        return MPV_ERROR_INVALID_PARAMETER; // exception pending
    ScopedUtfChars value(env, jvalue);
    if (!value.c_str())
        return MPV_ERROR_INVALID_PARAMETER;

    return mpv_set_option_string(mpv_instance->mpv, option.c_str(), value.c_str());
}

static int common_get_property(JNIEnv *env, mpv_handle* mpv, jstring jproperty, mpv_format format, void *output) {
    if (!mpv) {
        die(env, "get_property called but mpv is not initialized");
        return -1;
    }

    ScopedUtfChars prop(env, jproperty);
    if (!prop.c_str())
        return MPV_ERROR_INVALID_PARAMETER; // exception pending

    int result = mpv_get_property(mpv, prop.c_str(), format, output);
    if (result < 0)
        ALOGE("mpv_get_property(%s) format %d returned error %s", prop.c_str(), format, mpv_error_string(result));

    return result;
}

static int common_set_property(JNIEnv *env, mpv_handle* mpv, jstring jproperty, mpv_format format, void *value) {
    if (!mpv) {
        die(env, "set_property called but mpv is not initialized");
        return -1;
    }

    ScopedUtfChars prop(env, jproperty);
    if (!prop.c_str())
        return MPV_ERROR_INVALID_PARAMETER; // exception pending

    int result = mpv_set_property(mpv, prop.c_str(), format, value);
    if (result < 0)
        ALOGE("mpv_set_property(%s, %p) format %d returned error %s", prop.c_str(), value, format, mpv_error_string(result));

    return result;
}

jni_func(jobject, nativeGetPropertyInt, jlong instance, jstring jproperty) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    int64_t value = 0;
    if (common_get_property(env, mpv_instance->mpv, jproperty, MPV_FORMAT_INT64, &value) < 0)
        return nullptr;
    return env->NewObject(java_Integer, java_Integer_init, (jint)value);
}

jni_func(jobject, nativeGetPropertyLong, jlong instance, jstring jproperty) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    int64_t value = 0;
    if (common_get_property(env, mpv_instance->mpv, jproperty, MPV_FORMAT_INT64, &value) < 0)
        return nullptr;
    return env->NewObject(java_Long, java_Long_init, (jlong)value);
}

jni_func(jobject, nativeGetPropertyDouble, jlong instance, jstring jproperty) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    double value = 0;
    if (common_get_property(env, mpv_instance->mpv, jproperty, MPV_FORMAT_DOUBLE, &value) < 0)
        return nullptr;
    return env->NewObject(java_Double, java_Double_init, (jdouble)value);
}

jni_func(jobject, nativeGetPropertyBoolean, jlong instance, jstring jproperty) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    int value = 0;
    if (common_get_property(env, mpv_instance->mpv, jproperty, MPV_FORMAT_FLAG, &value) < 0)
        return nullptr;
    return env->NewObject(java_Boolean, java_Boolean_init, (jboolean)value);
}

jni_func(jstring, nativeGetPropertyString, jlong instance, jstring jproperty) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    char *value;
    if (common_get_property(env, mpv_instance->mpv, jproperty, MPV_FORMAT_STRING, &value) < 0)
        return nullptr;
    jstring jvalue = new_string_utf_safe(env, value);
    mpv_free(value);
    return jvalue;
}

jni_func(void, nativeSetPropertyInt, jlong instance, jstring jproperty, jint jvalue) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    int64_t value = static_cast<int64_t>(jvalue);
    common_set_property(env, mpv_instance->mpv, jproperty, MPV_FORMAT_INT64, &value);
}

jni_func(void, nativeSetPropertyDouble, jlong instance, jstring jproperty, jdouble jvalue) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    double value = static_cast<double>(jvalue);
    common_set_property(env, mpv_instance->mpv, jproperty, MPV_FORMAT_DOUBLE, &value);
}

jni_func(void, nativeSetPropertyBoolean, jlong instance, jstring jproperty, jboolean jvalue) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    int value = jvalue == JNI_TRUE ? 1 : 0;
    common_set_property(env, mpv_instance->mpv, jproperty, MPV_FORMAT_FLAG, &value);
}

jni_func(void, nativeSetPropertyString, jlong instance, jstring jproperty, jstring jvalue) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    ScopedUtfChars value_chars(env, jvalue);
    if (!value_chars.c_str())
        return; // exception pending
    const char *value = value_chars.c_str();
    common_set_property(env, mpv_instance->mpv, jproperty, MPV_FORMAT_STRING, &value);
}

jni_func(void, nativeObserveProperty, jlong instance, jstring jproperty, jint jformat) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    if (!mpv_instance->mpv) {
        die(env, "mpv is not initialized");
        return;
    }

    ScopedUtfChars prop(env, jproperty);
    if (!prop.c_str())
        return; // exception pending

    int result = mpv_observe_property(mpv_instance->mpv, 0, prop.c_str(), (mpv_format)jformat);
    if (result < 0)
        ALOGE("mpv_observe_property(%s) format %d returned error %s", prop.c_str(), jformat, mpv_error_string(result));
}
