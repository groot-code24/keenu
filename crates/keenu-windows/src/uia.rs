// SPDX-License-Identifier: GPL-3.0-or-later

//! The few COM interfaces Keenu uses that the `windows` crate offers only behind its largest features, declared here:
//! UI Automation's (`Win32_UI_Accessibility` nearly doubles the memory it takes to compile that crate) and the sound
//! volume's (whose `IMMDevice::Activate` needs the Variant features). Each interface lists its methods in the order of
//! its header up to the last one Keenu calls; the numbered ones only hold their places in the table and are never
//! called.

#![allow(non_snake_case, dead_code)]

use std::ffi::c_void;
use std::ptr::null_mut;

use windows::Win32::Foundation::HWND;
use windows_core::{
    BOOL, BSTR, GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, Result, interface,
};

pub const CUI_AUTOMATION: GUID = GUID::from_u128(0xff48dba4_60ef_4201_aa87_54103eef594e);
pub const TREE_SCOPE_DESCENDANTS: i32 = 4;
pub const UIA_INVOKE_PATTERN: i32 = 10000;
pub const UIA_VALUE_PATTERN: i32 = 10002;
pub const UIA_SELECTION_ITEM_PATTERN: i32 = 10010;
pub const UIA_TOGGLE_PATTERN: i32 = 10015;

#[interface("30cbe57d-d9d0-452a-ab13-7ac5ac4825ee")]
pub unsafe trait IUIAutomation: IUnknown {
    fn _1(&self) -> HRESULT;
    fn _2(&self) -> HRESULT;
    fn _3(&self) -> HRESULT;
    fn ElementFromHandle(&self, window: HWND, element: *mut *mut c_void) -> HRESULT;
    fn _5(&self) -> HRESULT;
    fn _6(&self) -> HRESULT;
    fn _7(&self) -> HRESULT;
    fn _8(&self) -> HRESULT;
    fn _9(&self) -> HRESULT;
    fn _10(&self) -> HRESULT;
    fn _11(&self) -> HRESULT;
    fn _12(&self) -> HRESULT;
    fn _13(&self) -> HRESULT;
    fn _14(&self) -> HRESULT;
    fn _15(&self) -> HRESULT;
    fn _16(&self) -> HRESULT;
    fn _17(&self) -> HRESULT;
    fn _18(&self) -> HRESULT;
    fn CreateTrueCondition(&self, condition: *mut *mut c_void) -> HRESULT;
}

#[interface("d22108aa-8ac5-49a5-837b-37bbb3d7591e")]
pub unsafe trait IUIAutomationElement: IUnknown {
    fn _1(&self) -> HRESULT;
    fn _2(&self) -> HRESULT;
    fn _3(&self) -> HRESULT;
    fn FindAll(&self, scope: i32, condition: *mut c_void, found: *mut *mut c_void) -> HRESULT;
    fn _5(&self) -> HRESULT;
    fn _6(&self) -> HRESULT;
    fn _7(&self) -> HRESULT;
    fn _8(&self) -> HRESULT;
    fn _9(&self) -> HRESULT;
    fn _10(&self) -> HRESULT;
    fn _11(&self) -> HRESULT;
    fn GetCurrentPatternAs(
        &self,
        pattern: i32,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> HRESULT;
    fn _13(&self) -> HRESULT;
    fn _14(&self) -> HRESULT;
    fn _15(&self) -> HRESULT;
    fn _16(&self) -> HRESULT;
    fn _17(&self) -> HRESULT;
    fn _18(&self) -> HRESULT;
    fn CurrentControlType(&self, kind: *mut i32) -> HRESULT;
    fn _20(&self) -> HRESULT;
    fn CurrentName(&self, name: *mut BSTR) -> HRESULT;
    fn _22(&self) -> HRESULT;
    fn _23(&self) -> HRESULT;
    fn _24(&self) -> HRESULT;
    fn _25(&self) -> HRESULT;
    fn CurrentIsEnabled(&self, enabled: *mut BOOL) -> HRESULT;
    fn _27(&self) -> HRESULT;
    fn _28(&self) -> HRESULT;
    fn _29(&self) -> HRESULT;
    fn _30(&self) -> HRESULT;
    fn _31(&self) -> HRESULT;
    fn _32(&self) -> HRESULT;
    fn CurrentIsPassword(&self, password: *mut BOOL) -> HRESULT;
}

#[interface("14314595-b4bc-4055-95f2-58f2e42c9855")]
pub unsafe trait IUIAutomationElementArray: IUnknown {
    fn Length(&self, length: *mut i32) -> HRESULT;
    fn GetElement(&self, index: i32, element: *mut *mut c_void) -> HRESULT;
}

#[interface("352ffba8-0973-437c-a61f-f64cafd81df9")]
pub unsafe trait IUIAutomationCondition: IUnknown {}

#[interface("fb377fbe-8ea6-46d5-9c73-6499642d3059")]
pub unsafe trait IUIAutomationInvokePattern: IUnknown {
    pub fn Invoke(&self) -> HRESULT;
}

#[interface("94cf8058-9b8d-4ab9-8bfd-4cd0a33c8c70")]
pub unsafe trait IUIAutomationTogglePattern: IUnknown {
    pub fn Toggle(&self) -> HRESULT;
}

#[interface("a8efa66a-0fda-421a-9194-38021f3578ea")]
pub unsafe trait IUIAutomationSelectionItemPattern: IUnknown {
    pub fn Select(&self) -> HRESULT;
}

#[interface("a94cd8b1-0844-4cd6-9d2d-640537ab39e9")]
pub unsafe trait IUIAutomationValuePattern: IUnknown {
    fn SetValue(&self, value: *const u16) -> HRESULT;
    fn CurrentValue(&self, value: *mut BSTR) -> HRESULT;
}

/// `IMMDevice`, for its `Activate`: the `windows` crate's needs its Variant features.
#[interface("d666063f-1587-4e43-81f1-b948e807363f")]
pub unsafe trait IMMDeviceActivate: IUnknown {
    pub fn Activate(
        &self,
        iid: *const GUID,
        context: u32,
        params: *const c_void,
        out: *mut *mut c_void,
    ) -> HRESULT;
}

/// The volume interface, in the order of `endpointvolume.h`.
#[interface("5cdf2c82-841e-4546-9722-0cf74078229a")]
pub unsafe trait IAudioEndpointVolume: IUnknown {
    fn _1(&self) -> HRESULT;
    fn _2(&self) -> HRESULT;
    fn _3(&self) -> HRESULT;
    fn _4(&self) -> HRESULT;
    pub fn SetMasterVolumeLevelScalar(&self, level: f32, context: *const GUID) -> HRESULT;
    fn _6(&self) -> HRESULT;
    pub fn GetMasterVolumeLevelScalar(&self, level: *mut f32) -> HRESULT;
    fn _8(&self) -> HRESULT;
    fn _9(&self) -> HRESULT;
    fn _10(&self) -> HRESULT;
    fn _11(&self) -> HRESULT;
    pub fn SetMute(&self, mute: BOOL, context: *const GUID) -> HRESULT;
}

/// The object a call hands back; an empty answer is an error, as UI Automation gives one for a missing pattern.
unsafe fn take<T: Interface>(call: impl FnOnce(*mut *mut c_void) -> HRESULT) -> Result<T> {
    let mut raw = null_mut();
    call(&mut raw).ok()?;
    if raw.is_null() {
        return Err(windows_core::Error::from_hresult(
            windows::Win32::Foundation::E_NOINTERFACE,
        ));
    }
    // SAFETY: the call succeeded and handed over one reference to an object of this type.
    Ok(unsafe { T::from_raw(raw) })
}

impl IUIAutomation {
    pub unsafe fn element_from_handle(&self, window: HWND) -> Result<IUIAutomationElement> {
        unsafe { take(|out| self.ElementFromHandle(window, out)) }
    }

    pub unsafe fn create_true_condition(&self) -> Result<IUIAutomationCondition> {
        unsafe { take(|out| self.CreateTrueCondition(out)) }
    }
}

impl IUIAutomationElement {
    pub unsafe fn find_all(
        &self,
        scope: i32,
        condition: &IUIAutomationCondition,
    ) -> Result<IUIAutomationElementArray> {
        unsafe { take(|out| self.FindAll(scope, condition.as_raw(), out)) }
    }

    pub unsafe fn pattern<T: Interface>(&self, pattern: i32) -> Result<T> {
        unsafe { take(|out| self.GetCurrentPatternAs(pattern, &T::IID, out)) }
    }

    pub unsafe fn control_type(&self) -> Result<i32> {
        let mut kind = 0;
        unsafe { self.CurrentControlType(&mut kind) }.ok()?;
        Ok(kind)
    }

    pub unsafe fn name(&self) -> Result<String> {
        let mut name = BSTR::default();
        unsafe { self.CurrentName(&mut name) }.ok()?;
        Ok(name.to_string())
    }

    pub unsafe fn is_enabled(&self) -> Result<bool> {
        let mut yes = BOOL::default();
        unsafe { self.CurrentIsEnabled(&mut yes) }.ok()?;
        Ok(yes.as_bool())
    }

    pub unsafe fn is_password(&self) -> Result<bool> {
        let mut yes = BOOL::default();
        unsafe { self.CurrentIsPassword(&mut yes) }.ok()?;
        Ok(yes.as_bool())
    }
}

impl IUIAutomationElementArray {
    pub unsafe fn length(&self) -> Result<i32> {
        let mut length = 0;
        unsafe { self.Length(&mut length) }.ok()?;
        Ok(length)
    }

    pub unsafe fn element(&self, index: i32) -> Result<IUIAutomationElement> {
        unsafe { take(|out| self.GetElement(index, out)) }
    }
}

impl IUIAutomationValuePattern {
    pub unsafe fn value(&self) -> Result<String> {
        let mut value = BSTR::default();
        unsafe { self.CurrentValue(&mut value) }.ok()?;
        Ok(value.to_string())
    }

    pub unsafe fn set_value(&self, text: &str) -> Result<()> {
        // A BSTR, which an empty one may be passed as null.
        let value = BSTR::from(text);
        let raw = if value.is_empty() {
            std::ptr::null()
        } else {
            value.as_ptr()
        };
        unsafe { self.SetValue(raw) }.ok()
    }
}
