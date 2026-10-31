// SPDX-License-Identifier: GPL-3.0-or-later

//! Native toasts through WinRT, shown under Keenu's own name and icon, with buttons that come back to
//! the running program.

use std::sync::Mutex;

use keenu_core::paths::{APP_USER_MODEL_ID, PRODUCT};
use keenu_platform::{Availability, NativeNotification, NotificationProvider, Result};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    NotificationSetting, ToastActivatedEventArgs, ToastNotification, ToastNotificationManager,
};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegSetValueExW,
};
use windows_core::{HSTRING, Interface, PCWSTR};

/// What a button click sends back: the notification's tag and the action's id.
pub type OnAction = Box<dyn Fn(String, String) + Send + Sync>;

pub struct Notifier {
    on_action: Option<std::sync::Arc<OnAction>>,
    /// Toasts keep their click handlers only while something holds them.
    recent: Mutex<Vec<ToastNotification>>,
}

impl Notifier {
    pub fn new(on_action: Option<OnAction>) -> Self {
        let _ = register_identity();
        Self {
            on_action: on_action.map(std::sync::Arc::new),
            recent: Mutex::new(Vec::new()),
        }
    }
}

/// Tells Windows who `Keenu.Desktop` is, so a toast shows the name even when there is no Start Menu
/// shortcut, as with the portable build.
pub fn register_identity() -> std::result::Result<(), String> {
    let path = HSTRING::from(format!(
        "Software\\Classes\\AppUserModelId\\{APP_USER_MODEL_ID}"
    ));
    let mut key = HKEY::default();
    // SAFETY: creates a key under this user's hive, closed below.
    unsafe {
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        if status != ERROR_SUCCESS {
            return Err(format!("could not register the app's name ({})", status.0));
        }
        let wide: Vec<u16> = PRODUCT.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
        let name = HSTRING::from("DisplayName");
        let status = RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(key);
        if status != ERROR_SUCCESS {
            return Err(format!("could not register the app's name ({})", status.0));
        }
    }
    Ok(())
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Not allowed in XML at all.
            c if c.is_control() && !matches!(c, '\n' | '\t') => {}
            c => out.push(c),
        }
    }
    out
}

/// The toast's XML. Button arguments are the action ids.
fn toast_xml(notification: &NativeNotification) -> String {
    let actions: String = notification
        .actions
        .iter()
        .map(|(id, label)| {
            format!(
                "<action content=\"{}\" arguments=\"{}\" activationType=\"foreground\"/>",
                escape(label),
                escape(id)
            )
        })
        .collect();
    let actions = if actions.is_empty() {
        String::new()
    } else {
        format!("<actions>{actions}</actions>")
    };
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual>{actions}</toast>",
        escape(&notification.title),
        escape(&notification.body),
    )
}

impl NotificationProvider for Notifier {
    fn availability(&self) -> Availability {
        let setting =
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_USER_MODEL_ID))
                .and_then(|notifier| notifier.Setting());
        match setting {
            Ok(NotificationSetting::Enabled) => Availability::Available,
            Ok(
                NotificationSetting::DisabledForUser | NotificationSetting::DisabledForApplication,
            ) => Availability::NeedsPermission(
                "Notifications are off for Keenu or for this user in Windows Settings.".into(),
            ),
            Ok(
                NotificationSetting::DisabledByGroupPolicy
                | NotificationSetting::DisabledByManifest,
            ) => Availability::Unsupported(
                "Notifications are turned off by a Windows policy.".into(),
            ),
            Ok(_) => Availability::Available,
            Err(err) => Availability::Unsupported(format!(
                "Windows notifications are unavailable: {}",
                err.message()
            )),
        }
    }

    fn show(&self, notification: &NativeNotification) -> Result<()> {
        let show = || -> windows_core::Result<ToastNotification> {
            let xml = XmlDocument::new()?;
            xml.LoadXml(&HSTRING::from(toast_xml(notification)))?;
            let toast = ToastNotification::CreateToastNotification(&xml)?;
            toast.SetTag(&HSTRING::from(tag(&notification.tag)))?;
            if let Some(on_action) = &self.on_action {
                let on_action = on_action.clone();
                let tag = notification.tag.clone();
                toast.Activated(&TypedEventHandler::new(
                    move |_, args: windows_core::Ref<'_, windows_core::IInspectable>| {
                        let chosen = args
                            .as_ref()
                            .and_then(|args| args.cast::<ToastActivatedEventArgs>().ok())
                            .and_then(|args| args.Arguments().ok())
                            .map(|arguments| arguments.to_string())
                            .unwrap_or_default();
                        on_action(tag.clone(), chosen);
                        Ok(())
                    },
                ))?;
            }
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_USER_MODEL_ID))?
                .Show(&toast)?;
            Ok(toast)
        };
        let toast =
            show().map_err(|err| format!("Could not show the notification: {}", err.message()))?;
        if let Ok(mut recent) = self.recent.lock() {
            recent.push(toast);
            let excess = recent.len().saturating_sub(16);
            recent.drain(..excess);
        }
        Ok(())
    }
}

/// Windows limits a toast's tag to 64 characters.
fn tag(value: &str) -> String {
    value.chars().take(64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str) -> NativeNotification {
        NativeNotification {
            tag: "test".into(),
            title: title.into(),
            body: "Body".into(),
            actions: vec![("a1".into(), "Open".into())],
        }
    }

    #[test]
    fn text_cannot_break_out_of_the_xml() {
        let xml = toast_xml(&note("</text><evil a=\"1\"> & 'x' \u{1}"));
        assert!(!xml.contains("<evil"));
        assert!(xml.contains("&lt;/text&gt;&lt;evil a=&quot;1&quot;&gt; &amp; &apos;x&apos; "));
        assert!(xml.contains("arguments=\"a1\""));
        // Windows' own parser must accept what is produced.
        let document = XmlDocument::new().unwrap();
        document.LoadXml(&HSTRING::from(xml)).unwrap();
    }

    #[test]
    fn tags_fit_what_windows_allows() {
        assert_eq!(tag(&"x".repeat(100)).len(), 64);
    }
}
