use super::ffi::*;
use crate::{HelperError, Result};
use std::{
    ffi::{CStr, CString},
    ptr,
};
struct Cf(Object);
impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}
/// Read the running signed image, not caller-supplied configuration or a mutable executable path.
pub fn peer_requirement(identifier: &str) -> Result<CString> {
    let mut code = ptr::null_mut();
    if unsafe { SecCodeCopySelf(0, &mut code) } != 0 || code.is_null() {
        return Err(unsigned());
    }
    let code = Cf(code);
    if unsafe { SecCodeCheckValidity(code.0, 0, ptr::null_mut()) } != 0 {
        return Err(unsigned());
    }
    let mut information = ptr::null_mut();
    if unsafe { SecCodeCopySigningInformation(code.0, 2, &mut information) } != 0
        || information.is_null()
    {
        return Err(unsigned());
    }
    let information = Cf(information);
    let team = unsafe { CFDictionaryGetValue(information.0, kSecCodeInfoTeamIdentifier) };
    if team.is_null() {
        return Err(unsigned());
    }
    let mut bytes = [0i8; 128];
    if unsafe { CFStringGetCString(team, bytes.as_mut_ptr(), bytes.len() as isize, 0x08000100) }
        == 0
    {
        return Err(unsigned());
    }
    let team = unsafe { CStr::from_ptr(bytes.as_ptr()) }
        .to_str()
        .map_err(|_| unsigned())?;
    if team.is_empty() || !team.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(unsigned());
    }
    let identifier_rule = if identifier == super::APP_ID {
        format!(
            "(identifier \"{}\" or identifier \"{}\")",
            super::APP_ID,
            super::CLI_ID
        )
    } else {
        format!("identifier \"{identifier}\"")
    };
    CString::new(format!("anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"{team}\" and {identifier_rule}"))
        .map_err(|_|unsigned())
}
fn unsigned() -> HelperError {
    HelperError::Unavailable("The macOS helper requires an app and helper signed by the same Developer ID team; an unsigned or ad-hoc local binary cannot authorize privileged operations".into())
}

/// Validate the sender through the XPC message audit token; never trust a PID lookup.
pub fn verify_message(message: Object, identifier: &str) -> Result<()> {
    let requirement = peer_requirement(identifier)?;
    let string =
        Cf(unsafe { CFStringCreateWithCString(ptr::null_mut(), requirement.as_ptr(), 0x08000100) });
    if string.0.is_null() {
        return Err(HelperError::Authentication);
    }
    let mut rule = ptr::null_mut();
    if unsafe { SecRequirementCreateWithString(string.0, 0, &mut rule) } != 0 || rule.is_null() {
        return Err(HelperError::Authentication);
    }
    let rule = Cf(rule);
    let mut peer = ptr::null_mut();
    if unsafe { SecCodeCreateWithXPCMessage(message, 0, &mut peer) } != 0 || peer.is_null() {
        return Err(HelperError::Authentication);
    }
    let peer = Cf(peer);
    if unsafe { SecCodeCheckValidity(peer.0, 0, rule.0) } != 0 {
        return Err(HelperError::Authentication);
    }
    Ok(())
}
