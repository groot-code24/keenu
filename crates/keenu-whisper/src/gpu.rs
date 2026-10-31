// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! The GPUs ggml found (through Vulkan, when built with it), and the one speech runs on.

use std::ffi::CStr;
use std::os::raw::c_int;

use whisper_rs::whisper_rs_sys as sys;

/// A GPU speech could run on, with its name for Settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Gpu {
    /// Its place among the GPUs, as whisper.cpp counts them (`gpu_device`).
    pub index: c_int,
    pub name: String,
    pub integrated: bool,
}

/// The GPUs ggml can run on, in its order.
pub fn list() -> Vec<Gpu> {
    let mut gpus = Vec::new();
    // SAFETY: ggml's device registry is built on first use and lives as long as the program; the
    // description is a C string it owns.
    unsafe {
        for i in 0..sys::ggml_backend_dev_count() {
            let device = sys::ggml_backend_dev_get(i);
            let integrated = match sys::ggml_backend_dev_type(device) {
                sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU => false,
                sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU => true,
                _ => continue,
            };
            let name = sys::ggml_backend_dev_description(device);
            let name = if name.is_null() {
                "GPU".to_owned()
            } else {
                CStr::from_ptr(name).to_string_lossy().trim().to_owned()
            };
            gpus.push(Gpu {
                index: gpus.len() as c_int,
                name,
                integrated,
            });
        }
    }
    gpus
}

/// The first discrete GPU, else none (the processor): an integrated one was slower than the CPU and
/// lost its device on the full model. `pinned` (GGML_VK_VISIBLE_DEVICES set) takes the first listed.
pub fn choose(gpus: &[Gpu], pinned: bool) -> Option<&Gpu> {
    if pinned {
        gpus.first()
    } else {
        gpus.iter().find(|gpu| !gpu.integrated)
    }
}

/// Whether the user picked the GPUs ggml may use, by their Vulkan numbers.
pub fn pinned() -> bool {
    std::env::var_os("GGML_VK_VISIBLE_DEVICES").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(index: c_int, name: &str, integrated: bool) -> Gpu {
        Gpu {
            index,
            name: name.into(),
            integrated,
        }
    }

    #[test]
    fn none_is_the_processor() {
        assert_eq!(choose(&[], false), None);
        assert_eq!(choose(&[], true), None);
    }

    #[test]
    fn takes_the_discrete_one() {
        let gpus = [
            gpu(0, "Intel(R) UHD Graphics 630 (CFL GT2)", true),
            gpu(1, "NVIDIA GeForce GTX 1650", false),
        ];
        assert_eq!(choose(&gpus, false), Some(&gpus[1]));
    }

    #[test]
    fn leaves_an_integrated_one_unless_picked() {
        let gpus = [gpu(0, "AMD Radeon Graphics (RADV RENOIR)", true)];
        assert_eq!(choose(&gpus, false), None);
        assert_eq!(choose(&gpus, true), Some(&gpus[0]));
    }
}
