#define UTIL_EXTERN
#include "jni_utils.h"

#include <jni.h>
#include <cstdlib>
#include <mutex>
#include <string>

bool acquire_jni_env(JavaVM *vm, JNIEnv **env)
{
    int ret = vm->GetEnv((void**)env, JNI_VERSION_1_6);

    if (ret == JNI_EDETACHED) {
        return vm->AttachCurrentThread(env, nullptr) == JNI_OK;
    }

    return ret == JNI_OK;
}

// Returns the length of the valid UTF-8 sequence starting at s, or 0 if the
// bytes at s are not valid UTF-8 (overlong forms, surrogates and code points
// above U+10FFFF are rejected). Never reads past a terminating NUL, since a
// NUL is never a valid continuation byte.
static size_t utf8_sequence_length(const unsigned char *s) {
    unsigned char c = s[0];
    if (c < 0x80)
        return 1;

    size_t len;
    unsigned char lo = 0x80, hi = 0xbf; // allowed range of the second byte
    if (c >= 0xc2 && c <= 0xdf) {
        len = 2;
    } else if (c >= 0xe0 && c <= 0xef) {
        len = 3;
        if (c == 0xe0)
            lo = 0xa0; // overlong
        else if (c == 0xed)
            hi = 0x9f; // surrogates
    } else if (c >= 0xf0 && c <= 0xf4) {
        len = 4;
        if (c == 0xf0)
            lo = 0x90; // overlong
        else if (c == 0xf4)
            hi = 0x8f; // > U+10FFFF
    } else {
        return 0;
    }

    if (s[1] < lo || s[1] > hi)
        return 0;
    for (size_t i = 2; i < len; i++) {
        if ((s[i] & 0xc0) != 0x80)
            return 0;
    }
    return len;
}

jstring new_string_utf_safe(JNIEnv *env, const char *str) {
    if (!str)
        return nullptr;

    const unsigned char *s = reinterpret_cast<const unsigned char*>(str);
    size_t i = 0;
    while (s[i]) {
        size_t n = utf8_sequence_length(s + i);
        if (n == 0)
            break;
        i += n;
    }
    if (!s[i])
        return env->NewStringUTF(str);

    // Invalid byte at i: copy the valid prefix and replace each invalid byte.
    std::string sanitized(str, i);
    while (s[i]) {
        size_t n = utf8_sequence_length(s + i);
        if (n == 0) {
            sanitized += '?';
            i++;
        } else {
            sanitized.append(str + i, n);
            i += n;
        }
    }
    return env->NewStringUTF(sanitized.c_str());
}

static std::once_flag init_flag;

// Apparently it's considered slow to FindClass and GetMethodID every time we need them,
// so let's have a nice cache here
void init_methods_cache(JNIEnv *env) {
    std::call_once(init_flag, [env]() {
        auto find_and_ref = [&](const char* name) -> jclass {
            jclass localClass = env->FindClass(name);
            auto globalRef = reinterpret_cast<jclass>(env->NewGlobalRef(localClass));
            env->DeleteLocalRef(localClass);
            return globalRef;
        };

        java_Integer = find_and_ref("java/lang/Integer");
        java_Integer_init = env->GetMethodID(java_Integer, "<init>", "(I)V");
        java_Long = find_and_ref("java/lang/Long");
        java_Long_init = env->GetMethodID(java_Long, "<init>", "(J)V");
        java_Double = find_and_ref("java/lang/Double");
        java_Double_init = env->GetMethodID(java_Double, "<init>", "(D)V");
        java_Boolean = find_and_ref("java/lang/Boolean");
        java_Boolean_init = env->GetMethodID(java_Boolean, "<init>", "(Z)V");

        mpv_MPVLib = find_and_ref("dev/jdtech/mpv/MPVLib");
        mpv_MPVLib_eventProperty_S  = env->GetMethodID(mpv_MPVLib, "eventProperty", "(Ljava/lang/String;)V"); // eventProperty(String)
        mpv_MPVLib_eventProperty_Sb = env->GetMethodID(mpv_MPVLib, "eventProperty", "(Ljava/lang/String;Z)V"); // eventProperty(String, boolean)
        mpv_MPVLib_eventProperty_Sl = env->GetMethodID(mpv_MPVLib, "eventProperty", "(Ljava/lang/String;J)V"); // eventProperty(String, long)
        mpv_MPVLib_eventProperty_Sd = env->GetMethodID(mpv_MPVLib, "eventProperty", "(Ljava/lang/String;D)V"); // eventProperty(String, double)
        mpv_MPVLib_eventProperty_SS = env->GetMethodID(mpv_MPVLib, "eventProperty", "(Ljava/lang/String;Ljava/lang/String;)V"); // eventProperty(String, String)
        mpv_MPVLib_event = env->GetMethodID(mpv_MPVLib, "event", "(I)V"); // event(int)
        mpv_MPVLib_logMessage_SiS = env->GetMethodID(mpv_MPVLib, "logMessage", "(Ljava/lang/String;ILjava/lang/String;)V"); // logMessage(String, int, String)
    });
}
