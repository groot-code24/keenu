// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Vulkan loaded only when there: defining ggml's three direct calls here keeps libvulkan.so.1 off the
//! needed libraries. Without it (or with `--cpu`) it answers like a loader with no driver, so ggml uses the CPU.

use std::ffi::{CStr, c_char, c_void};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

type Handle = *mut c_void;

const VK_SUCCESS: i32 = 0;
const VK_ERROR_INCOMPATIBLE_DRIVER: i32 = -9;
/// Vulkan 1.0, older than the 1.2 ggml needs: it gives up on Vulkan right away.
const VK_API_VERSION_1_0: u32 = 1 << 22;

static DISABLED: AtomicBool = AtomicBool::new(false);

/// Never loads Vulkan; call before ggml first looks for devices.
pub fn disable() {
    DISABLED.store(true, Ordering::Relaxed);
}

/// The system's Vulkan loader (its dlopen handle, as an address), when it can be loaded.
fn library() -> Option<usize> {
    static LIBRARY: OnceLock<Option<usize>> = OnceLock::new();
    *LIBRARY.get_or_init(|| {
        if DISABLED.load(Ordering::Relaxed) {
            return None;
        }
        // SAFETY: dlopen takes a C string; the library stays loaded for good.
        let library = unsafe {
            libc::dlopen(
                c"libvulkan.so.1".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            )
        };
        (!library.is_null()).then_some(library as usize)
    })
}

/// One of the loader's own functions.
fn real(name: &CStr) -> Option<*mut c_void> {
    // SAFETY: a handle dlopen gave, and a C string.
    let symbol = unsafe { libc::dlsym(library()? as Handle, name.as_ptr()) };
    (!symbol.is_null()).then_some(symbol)
}

/// # Safety
/// Called by ggml as Vulkan's own: `name` is a C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vkGetInstanceProcAddr(
    instance: Handle,
    name: *const c_char,
) -> *const c_void {
    type Fn = unsafe extern "C" fn(Handle, *const c_char) -> *const c_void;
    match real(c"vkGetInstanceProcAddr") {
        // SAFETY: the loader's function of this name, with the caller's arguments.
        Some(real) => unsafe { std::mem::transmute::<*mut c_void, Fn>(real)(instance, name) },
        // SAFETY: as above.
        None => no_driver(unsafe { CStr::from_ptr(name) }),
    }
}

/// # Safety
/// As Vulkan's own; only reached once a device was found, so with the loader there.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vkGetPhysicalDeviceFeatures2(device: Handle, features: *mut c_void) {
    type Fn = unsafe extern "C" fn(Handle, *mut c_void);
    let real = real(c"vkGetPhysicalDeviceFeatures2").unwrap_or_else(|| std::process::abort());
    // SAFETY: the loader's function of this name, with the caller's arguments.
    unsafe { std::mem::transmute::<*mut c_void, Fn>(real)(device, features) }
}

/// # Safety
/// As above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vkCmdCopyBuffer(
    commands: Handle,
    source: Handle,
    destination: Handle,
    count: u32,
    regions: *const c_void,
) {
    type Fn = unsafe extern "C" fn(Handle, Handle, Handle, u32, *const c_void);
    let real = real(c"vkCmdCopyBuffer").unwrap_or_else(|| std::process::abort());
    // SAFETY: as above.
    unsafe {
        std::mem::transmute::<*mut c_void, Fn>(real)(commands, source, destination, count, regions)
    }
}

/// The few functions a loader has before any driver, answering that there is none.
fn no_driver(name: &CStr) -> *const c_void {
    match name.to_bytes() {
        b"vkEnumerateInstanceVersion" => enumerate_instance_version as *const c_void,
        b"vkCreateInstance" => create_instance as *const c_void,
        b"vkEnumerateInstanceExtensionProperties" => enumerate_extensions as *const c_void,
        b"vkEnumerateInstanceLayerProperties" => enumerate_layers as *const c_void,
        _ => std::ptr::null(),
    }
}

/// Reports Vulkan 1.0, too old for ggml, so it stops looking.
unsafe extern "C" fn enumerate_instance_version(version: *mut u32) -> i32 {
    // SAFETY: Vulkan's caller passes somewhere to write the version.
    unsafe { version.write(VK_API_VERSION_1_0) };
    VK_SUCCESS
}

/// Fails as a loader does when no driver fits.
unsafe extern "C" fn create_instance(_: *const c_void, _: *const c_void, _: *mut c_void) -> i32 {
    VK_ERROR_INCOMPATIBLE_DRIVER
}

/// Lists no instance extensions.
unsafe extern "C" fn enumerate_extensions(
    _: *const c_char,
    count: *mut u32,
    _: *mut c_void,
) -> i32 {
    // SAFETY: Vulkan's caller passes somewhere to write the count.
    unsafe { count.write(0) };
    VK_SUCCESS
}

/// Lists no layers.
unsafe extern "C" fn enumerate_layers(count: *mut u32, _: *mut c_void) -> i32 {
    // SAFETY: as above.
    unsafe { count.write(0) };
    VK_SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_driver_vulkan_is_too_old_and_empty() {
        assert!(no_driver(c"vkCreateDevice").is_null());
        type Version = unsafe extern "C" fn(*mut u32) -> i32;
        // SAFETY: the function no_driver returns for this name.
        let version: Version =
            unsafe { std::mem::transmute(no_driver(c"vkEnumerateInstanceVersion")) };
        let mut found = 0;
        assert_eq!(unsafe { version(&mut found) }, VK_SUCCESS);
        // Older than 1.2.
        assert!(found < (1 << 22 | 2 << 12));
        type Create = unsafe extern "C" fn(*const c_void, *const c_void, *mut c_void) -> i32;
        // SAFETY: as above.
        let create: Create = unsafe { std::mem::transmute(no_driver(c"vkCreateInstance")) };
        let failed = unsafe { create(std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) };
        assert_eq!(failed, VK_ERROR_INCOMPATIBLE_DRIVER);
    }
}
