#pragma once

#include <atomic>
#include <mutex>
#include <pthread.h>
#include <jni.h>
#include <mpv/client.h>

// struct to hold the mpv_handle and other related data
struct MPVInstance {
    mpv_handle *mpv = nullptr;
    JavaVM *vm = nullptr;
    pthread_t event_thread_id = 0;
    std::atomic<bool> event_thread_request_exit{false};

    jobject javaObject = nullptr;

    // guards surface and the "wid" option (attach, detach, destroy)
    std::mutex surface_mutex;
    jobject surface = nullptr;
};
