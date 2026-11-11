//! Another app's preferences, through CFPreferences: the same API (and the same
//! cfprefsd cache) the game's own launcher reads them with, so a value written
//! here is what the game sees at its next start. Writing the plist file
//! directly would not be: cfprefsd may hold an older copy in memory.
//!
//! Only works outside the App Sandbox, which this launcher is.

use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;
use core_foundation_sys::preferences::{
    CFPreferencesAppSynchronize, CFPreferencesCopyAppValue, CFPreferencesSetAppValue,
};
use std::ptr;

/// A string value, or `None` when the key is missing or holds anything else
/// (the game's launcher ignores a value that is not a string, too).
pub fn get_string(domain: &str, key: &str) -> Option<String> {
    let (key, domain) = (CFString::new(key), CFString::new(domain));
    // SAFETY: Copy… returns a +1 reference or null; wrap_under_create_rule takes it over.
    let value = unsafe { CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), domain.as_concrete_TypeRef()) };
    if value.is_null() {
        return None;
    }
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    value.downcast::<CFString>().map(|s| s.to_string())
}

/// Sets a string, or removes the key with `None`. Call [`sync`] after a batch.
pub fn set_string(domain: &str, key: &str, value: Option<&str>) {
    let (key, domain) = (CFString::new(key), CFString::new(domain));
    let value = value.map(CFString::new);
    let value_ref = value.as_ref().map_or(ptr::null(), |v| v.as_CFTypeRef());
    // SAFETY: all three references are alive for the call; a null value removes the key.
    unsafe { CFPreferencesSetAppValue(key.as_concrete_TypeRef(), value_ref, domain.as_concrete_TypeRef()) };
}

/// Writes the domain's pending changes to disk; false when cfprefsd refused.
pub fn sync(domain: &str) -> bool {
    let domain = CFString::new(domain);
    unsafe { CFPreferencesAppSynchronize(domain.as_concrete_TypeRef()) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_read_remove() {
        // A domain of our own, removed again at the end.
        let domain = "dev.nxlauncher.tests";
        set_string(domain, "launcher.DM_TEST", Some("30"));
        assert!(sync(domain));
        assert_eq!(get_string(domain, "launcher.DM_TEST").as_deref(), Some("30"));
        set_string(domain, "launcher.DM_TEST", None);
        assert!(sync(domain));
        assert_eq!(get_string(domain, "launcher.DM_TEST"), None);
    }
}
