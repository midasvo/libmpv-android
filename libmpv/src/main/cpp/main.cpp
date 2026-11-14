#include <jni.h>
#include <cstdlib>
#include <cstdio>
#include <ctime>
#include <clocale>
#include <atomic>

#include <mpv/client.h>

#include <pthread.h>

extern "C" {
    #include <libavcodec/jni.h>
}

#include "log.h"
#include "jni_utils.h"
#include "event.h"
#include "globals.h"

#define ARRAYLEN(a) (sizeof(a)/sizeof(a[0]))

// Update the JNI functions to accept a jlong parameter, which will be a
// pointer to the MPVInstance
extern "C" {
    jni_func(jlong, nativeCreate, jobject thiz, jobject appctx);
    jni_func(void, nativeInit, jlong instance);
    jni_func(void, nativeDestroy, jlong instance);
    jni_func(jint, nativeCommand, jlong instance, jobjectArray jarray);
    jni_func(jint, nativeRequestLogMessages, jlong instance, jstring jlevel);
};

static void prepare_environment(JNIEnv *env, MPVInstance* instance) {
    setlocale(LC_NUMERIC, "C");

    if (env->GetJavaVM(&instance->vm) == JNI_OK && instance->vm)
        av_jni_set_java_vm(instance->vm, nullptr);

    if (instance->appCtx)
        av_jni_set_android_app_ctx(instance->appCtx, nullptr);

    init_methods_cache(env);
}

jni_func(jlong, nativeCreate, jobject thiz, jobject appctx) {
    auto instance = new MPVInstance();
    instance->event_thread_id = 0;
    instance->event_thread_request_exit = false;
    instance->javaObject = env->NewGlobalRef(thiz);
    instance->appCtx = env->NewGlobalRef(appctx);
    prepare_environment(env, instance);

    instance->mpv = mpv_create();
    if (!instance->mpv) {
        env->DeleteGlobalRef(instance->appCtx);
        env->DeleteGlobalRef(instance->javaObject);
        delete instance;
        return 0;
    }

    mpv_request_log_messages(instance->mpv, "v");
    return reinterpret_cast<jlong>(instance);
}

jni_func(void, nativeInit, jlong instance) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    if (!mpv_instance->mpv) {
        die(env, "mpv is not created");
        return;
    }

    if (mpv_initialize(mpv_instance->mpv) < 0) {
        die(env, "mpv init failed");
        return;
    }

    mpv_instance->event_thread_request_exit = false;
    if (pthread_create(&mpv_instance->event_thread_id, nullptr, event_thread, mpv_instance) != 0) {
        die(env, "thread create failed");
        return;
    }
    pthread_setname_np(mpv_instance->event_thread_id, "event_thread");
}

jni_func(void, nativeDestroy, jlong instance) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    if (!mpv_instance->mpv) {
        ALOGV("Cannot destroy mpv: mpv is not initialized");
        return;
    }

    mpv_instance->event_thread_request_exit = true;
    mpv_wakeup(mpv_instance->mpv);

    if (mpv_instance->event_thread_id != 0) {
        pthread_join(mpv_instance->event_thread_id, nullptr);
        mpv_instance->event_thread_id = 0;
    }

    mpv_terminate_destroy(mpv_instance->mpv);
    if (mpv_instance->surface) {
        env->DeleteGlobalRef(mpv_instance->surface);
        mpv_instance->surface = nullptr;
    }
    env->DeleteGlobalRef(mpv_instance->appCtx);
    env->DeleteGlobalRef(mpv_instance->javaObject);
    delete mpv_instance;
}

jni_func(jint, nativeCommand, jlong instance, jobjectArray jarray) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    if (!mpv_instance->mpv) {
        ALOGE("Cannot run command: mpv is not initialized");
        return MPV_ERROR_UNINITIALIZED;
    }

    const char *arguments[128] = { nullptr };
    jstring stringRefs[128] = { nullptr };

    int len = jarray ? env->GetArrayLength(jarray) : 0;

    if (!jarray || len >= (int) ARRAYLEN(arguments)) {
        ALOGE("Cannot run command: no or too many arguments");
        return MPV_ERROR_INVALID_PARAMETER;
    }

    int result = 0;
    for (int i = 0; i < len; ++i) {
        stringRefs[i] = (jstring)env->GetObjectArrayElement(jarray, i);
        if (!stringRefs[i]) {
            ALOGE("Cannot run command: argument %d is null", i);
            result = MPV_ERROR_INVALID_PARAMETER;
            break;
        }
        arguments[i] = env->GetStringUTFChars(stringRefs[i], nullptr);
        if (!arguments[i]) {
            // OutOfMemoryError is pending and will be thrown on return
            result = MPV_ERROR_NOMEM;
            break;
        }
    }

    if (result == 0)
        result = mpv_command(mpv_instance->mpv, arguments);

    for (int i = 0; i < len; ++i) {
        if (stringRefs[i]) {
            if (arguments[i])
                env->ReleaseStringUTFChars(stringRefs[i], arguments[i]);
            env->DeleteLocalRef(stringRefs[i]);
        }
    }

    return result;
}

jni_func(jint, nativeRequestLogMessages, jlong instance, jstring jlevel) {
    auto mpv_instance = reinterpret_cast<MPVInstance*>(instance);
    if (!mpv_instance->mpv) {
        die(env, "mpv is not initialized");
        return 0;
    }

    const char *level = env->GetStringUTFChars(jlevel, nullptr);
    int result = mpv_request_log_messages(mpv_instance->mpv, level);
    env->ReleaseStringUTFChars(jlevel, level);

    return result;
}
