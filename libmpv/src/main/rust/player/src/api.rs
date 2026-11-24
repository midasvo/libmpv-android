//! The static methods of `dev.jdtech.mpv.internal.Native`: structured values
//! (`mpv_node`), asynchronous calls, property observation by id, and custom
//! stream protocols. Kotlin's public API (MpvNodes.kt, MpvAsync.kt,
//! MpvFlows.kt, MpvStreams.kt) is built on these.
//!
//! Like `exports`, every function runs through `exports::entry`. An unknown
//! or destroyed handle throws `IllegalStateException`; an mpv error throws
//! `dev.jdtech.mpv.MpvException` with mpv's code. Nodes cross JNI encoded as
//! `byte[]` (see `node`).

use std::ffi::{CStr, c_void};
use std::ptr;

use jni_sys::{JNIEnv, jbyteArray, jclass, jint, jlong, jobject, jstring};

use crate::exports::{entry, instance};
use crate::ffi::*;
use crate::instance::{Failure, Result};
use crate::jni::Env;
use crate::node::{self, NodeError, OwnedNode};

/// The node Kotlin sent, decoded for mpv.
fn decode(env: &Env, bytes: jbyteArray, operation: &str) -> Result<OwnedNode> {
    let bytes = env.byte_array(bytes)?;
    OwnedNode::decode(&bytes).map_err(|e| {
        let why = match e {
            NodeError::TooDeep => "node nested too deeply",
            NodeError::Malformed => "malformed node",
        };
        Failure::mpv(MPV_ERROR_INVALID_PARAMETER, format!("{operation} ({why})"))
    })
}

/// Encodes a node mpv returned and frees it with `mpv_free_node_contents`.
///
/// # Safety
/// `node` was set by a successful mpv call and not freed yet.
unsafe fn take_node(env: &Env, mut node: mpv_node, operation: &str) -> Result<jbyteArray> {
    let mut out = Vec::new();
    // SAFETY: the caller's contract
    let encoded = unsafe { node::encode(&node, &mut out) };
    // SAFETY: mpv set it, and it is freed once
    unsafe { mpv_free_node_contents(&raw mut node) };
    encoded.map_err(|_| {
        Failure::mpv(
            MPV_ERROR_GENERIC,
            format!("{operation} (result not representable)"),
        )
    })?;
    Ok(env.new_byte_array(&out)?.into_raw())
}

fn check(r: i32, operation: impl FnOnce() -> String) -> Result<()> {
    if r < 0 {
        Err(Failure::mpv(r, operation()))
    } else {
        Ok(())
    }
}

fn lossy(s: &CStr) -> std::borrow::Cow<'_, str> {
    s.to_string_lossy()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_getPropertyNode(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    name: jstring,
) -> jbyteArray {
    entry(env, ptr::null_mut(), |env| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        let name = env.c_string(name)?;
        let mut node = mpv_node::NONE;
        // SAFETY: an admitted call; MPV_FORMAT_NODE takes an mpv_node*
        let r = unsafe {
            mpv_get_property(
                active.mpv(),
                name.as_ptr(),
                MPV_FORMAT_NODE,
                (&raw mut node).cast::<c_void>(),
            )
        };
        drop(active);
        let op = || format!("getPropertyNode({})", lossy(&name));
        check(r, op)?;
        // SAFETY: mpv set `node`
        unsafe { take_node(env, node, &op()) }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_setPropertyNode(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    name: jstring,
    value: jbyteArray,
) {
    entry(env, (), |env| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        let name = env.c_string(name)?;
        let op = || format!("setPropertyNode({})", lossy(&name));
        let mut value = decode(env, value, &op())?;
        // SAFETY: an admitted call; MPV_FORMAT_NODE takes an mpv_node*, valid
        // while `value` lives
        let r = unsafe {
            mpv_set_property(
                active.mpv(),
                name.as_ptr(),
                MPV_FORMAT_NODE,
                value.as_mut_ptr().cast::<c_void>(),
            )
        };
        check(r, op)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_commandNode(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    args: jbyteArray,
) -> jbyteArray {
    entry(env, ptr::null_mut(), |env| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        let mut args = decode(env, args, "commandNode")?;
        let mut result = mpv_node::NONE;
        // SAFETY: an admitted call; `args` is valid while it lives, and
        // `result` is set only on success
        let r = unsafe { mpv_command_node(active.mpv(), args.as_mut_ptr(), &raw mut result) };
        drop(active);
        check(r, || "commandNode".into())?;
        // SAFETY: mpv set `result`
        unsafe { take_node(env, result, "commandNode") }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_commandNodeAsync(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    id: jlong,
    args: jbyteArray,
) {
    entry(env, (), |env| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        let mut args = decode(env, args, "commandAsync")?;
        // SAFETY: an admitted call; mpv parses `args` before it returns
        let r = unsafe { mpv_command_node_async(active.mpv(), id as u64, args.as_mut_ptr()) };
        check(r, || "commandAsync".into())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_abortAsyncCommand(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    id: jlong,
) {
    entry(env, (), |_| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        // SAFETY: an admitted call
        unsafe { mpv_abort_async_command(active.mpv(), id as u64) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_getPropertyAsync(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    id: jlong,
    name: jstring,
) {
    entry(env, (), |env| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        let name = env.c_string(name)?;
        // SAFETY: an admitted call and a NUL-terminated name
        let r = unsafe {
            mpv_get_property_async(active.mpv(), id as u64, name.as_ptr(), MPV_FORMAT_NODE)
        };
        check(r, || format!("getPropertyAsync({})", lossy(&name)))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_setPropertyAsync(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    id: jlong,
    name: jstring,
    value: jbyteArray,
) {
    entry(env, (), |env| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        let name = env.c_string(name)?;
        let op = || format!("setPropertyAsync({})", lossy(&name));
        let mut value = decode(env, value, &op())?;
        // SAFETY: an admitted call; mpv copies the value before it returns
        let r = unsafe {
            mpv_set_property_async(
                active.mpv(),
                id as u64,
                name.as_ptr(),
                MPV_FORMAT_NODE,
                value.as_mut_ptr().cast::<c_void>(),
            )
        };
        check(r, op)
    })
}

/// The formats a property can be observed with here; mpv accepts more, but
/// these are the ones `onPropertyChange` can deliver.
fn observable(format: jint) -> bool {
    matches!(
        format,
        MPV_FORMAT_NONE
            | MPV_FORMAT_STRING
            | MPV_FORMAT_FLAG
            | MPV_FORMAT_INT64
            | MPV_FORMAT_DOUBLE
            | MPV_FORMAT_NODE
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_observeProperty(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    id: jlong,
    name: jstring,
    format: jint,
) {
    entry(env, (), |env| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        let name = env.c_string(name)?;
        // non-positive ids are MPVLib.observeProperty's (see `observations`)
        if id <= 0 || !observable(format) {
            return Err(Failure::mpv(
                MPV_ERROR_INVALID_PARAMETER,
                format!("observe({}) with id {id} and format {format}", lossy(&name)),
            ));
        }
        // SAFETY: an admitted call and a NUL-terminated name
        let r = unsafe { mpv_observe_property(active.mpv(), id as u64, name.as_ptr(), format) };
        check(r, || {
            format!("mpv_observe_property({}) format {format}", lossy(&name))
        })
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_unobserveProperty(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    id: jlong,
) -> jint {
    entry(env, MPV_ERROR_INVALID_PARAMETER, |_| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        if id <= 0 {
            return Ok(MPV_ERROR_INVALID_PARAMETER); // MPVLib.observeProperty's: by name only
        }
        // SAFETY: an admitted call
        Ok(unsafe { mpv_unobserve_property(active.mpv(), id as u64) })
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_unobserveLegacyProperty(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    name: jstring,
) -> jint {
    entry(env, MPV_ERROR_INVALID_PARAMETER, |env| {
        let instance = instance(handle)?;
        let name = env.c_string(name)?;
        instance.unobserve_legacy(&name)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_internal_Native_addStreamProtocol(
    env: *mut JNIEnv,
    _cls: jclass,
    handle: jlong,
    protocol: jstring,
    adapter: jobject,
) {
    entry(env, (), |env| {
        let instance = instance(handle)?;
        let protocol = env.c_string(protocol)?;
        instance.add_stream_protocol(env, &protocol, adapter)
    })
}
