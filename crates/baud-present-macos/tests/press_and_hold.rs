//! **A held key repeats: press-and-hold is registered off.**
//! `launch::register_defaults` puts `ApplePressAndHoldEnabled = NO` in this
//! process's registration domain, and this reads it back from there
//! through `NSUserDefaults`. Reading and registering defaults opens no
//! window, and the registration domain is volatile: nothing is written to
//! disk.
//!
//! One test, because the registration is process-wide and a second test
//! reading the key could run before or after it.
//!
//! Break: an empty `register_defaults` body leaves the key unregistered,
//! and the read after it is `None`.
#![cfg(target_os = "macos")]

use baud_present_macos::launch::{PRESS_AND_HOLD, register_defaults};
use macos_liquid_glass::objc2::runtime::AnyObject;
use macos_liquid_glass::objc2_foundation::{NSNumber, NSString, NSUserDefaults};

/// `key` as this process's registration domain holds it: `None` when
/// nothing registered it.
fn registered(key: &str) -> Option<bool> {
    let domain = NSUserDefaults::standardUserDefaults()
        .volatileDomainForName(&NSString::from_str("NSRegistrationDomain"));
    let obj: &AnyObject = &*domain.objectForKey(&NSString::from_str(key))?;
    Some(obj.downcast_ref::<NSNumber>()?.as_bool())
}

#[test]
fn press_and_hold_is_registered_off_before_any_window() {
    // Control: the read sees nothing before the registration, and a key
    // nobody registered reads `None` after it.
    assert_eq!(
        registered(PRESS_AND_HOLD),
        None,
        "registered before the call"
    );
    register_defaults();
    assert_eq!(
        registered(PRESS_AND_HOLD),
        Some(false),
        "{PRESS_AND_HOLD} = NO is not in the registration domain"
    );
    assert_eq!(registered("BaudNeverRegistered"), None);
}
