#define UTIL_EXTERN
#include "jni_utils.h"

#include <jni.h>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <vector>

bool acquire_jni_env(JavaVM *vm, JNIEnv **env)
{
    int ret = vm->GetEnv((void**)env, JNI_VERSION_1_6);

    if (ret == JNI_EDETACHED) {
        return vm->AttachCurrentThread(env, nullptr) == JNI_OK;
    }

    return ret == JNI_OK;
}

// Decodes the UTF-8 sequence starting at s: returns its length and stores the
// code point in *cp, or returns 0 if the bytes at s are not valid UTF-8
// (overlong forms, surrogates and code points above U+10FFFF are rejected).
// Never reads past a terminating NUL, since a NUL is never a valid
// continuation byte.
static size_t utf8_decode(const unsigned char *s, uint32_t *cp) {
    unsigned char c = s[0];
    if (c < 0x80) {
        *cp = c;
        return 1;
    }

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

    uint32_t value = c & (0x7f >> len); // payload bits of the lead byte
    for (size_t i = 1; i < len; i++)
        value = (value << 6) | (s[i] & 0x3f);
    *cp = value;
    return len;
}

jstring new_string_utf_safe(JNIEnv *env, const char *str) {
    if (!str)
        return nullptr;

    // Decode to UTF-16 ourselves and use NewString: NewStringUTF expects
    // Modified UTF-8, which encodes supplementary characters differently.
    const unsigned char *s = reinterpret_cast<const unsigned char*>(str);
    std::vector<jchar> units;
    units.reserve(strlen(str));
    size_t i = 0;
    while (s[i]) {
        uint32_t cp;
        size_t n = utf8_decode(s + i, &cp);
        if (n == 0) {
            units.push_back('?');
            i++;
            continue;
        }
        if (cp >= 0x10000) {
            cp -= 0x10000;
            units.push_back(static_cast<jchar>(0xd800 + (cp >> 10)));
            units.push_back(static_cast<jchar>(0xdc00 + (cp & 0x3ff)));
        } else {
            units.push_back(static_cast<jchar>(cp));
        }
        i += n;
    }

    // never hand NewString a null pointer, even for an empty string
    static const jchar empty = 0;
    const jchar *data = units.empty() ? &empty : units.data();
    return env->NewString(data, static_cast<jsize>(units.size()));
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
