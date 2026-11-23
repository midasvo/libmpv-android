#include "log.h"

#include "globals.h"
#include "jni_utils.h"

void die(JNIEnv *env, const char *msg)
{
    ALOGE("%s", msg);
    throw_java_exception(env, msg);
}

void throw_java_exception(JNIEnv *env, const char *msg)
{
    throw_java_exception(env, "java/lang/RuntimeException", msg);
}

void throw_java_exception(JNIEnv *env, const char *class_name, const char *msg)
{
    jclass exceptionClass = env->FindClass(class_name);
    if (exceptionClass == nullptr) {
        return;
    }

    env->ThrowNew(exceptionClass, msg);

    env->DeleteLocalRef(exceptionClass);
}
