//! Narrow bindings to public macOS SDK APIs (minimum deployment target 13.0).
use block2::Block;
use std::ffi::{c_char, c_void};
pub type Object = *mut c_void;
#[link(name = "System")]
unsafe extern "C" {
    pub fn xpc_connection_create_mach_service(
        name: *const c_char,
        queue: Object,
        flags: u64,
    ) -> Object;
    pub fn xpc_connection_set_peer_code_signing_requirement(
        connection: Object,
        requirement: *const c_char,
    ) -> i32;
    pub fn xpc_connection_set_event_handler(connection: Object, handler: &Block<dyn Fn(Object)>);
    pub fn xpc_connection_resume(connection: Object);
    pub fn xpc_connection_cancel(connection: Object);
    pub fn xpc_connection_send_message_with_reply(
        connection: Object,
        message: Object,
        queue: Object,
        handler: &Block<dyn Fn(Object)>,
    );
    pub fn xpc_connection_send_message(connection: Object, message: Object);
    pub fn xpc_connection_send_barrier(connection: Object, barrier: &Block<dyn Fn()>);
    pub fn xpc_connection_get_euid(connection: Object) -> u32;
    pub fn xpc_connection_get_pid(connection: Object) -> i32;
    pub fn xpc_dictionary_create(
        keys: *const *const c_char,
        values: *const Object,
        count: usize,
    ) -> Object;
    pub fn xpc_dictionary_create_reply(message: Object) -> Object;
    pub fn xpc_dictionary_set_data(
        object: Object,
        key: *const c_char,
        value: *const c_void,
        length: usize,
    );
    pub fn xpc_dictionary_get_data(
        object: Object,
        key: *const c_char,
        length: *mut usize,
    ) -> *const c_void;
    pub fn xpc_get_type(object: Object) -> *const c_void;
    pub fn xpc_retain(object: Object) -> Object;
    pub fn xpc_release(object: Object);
    pub static _xpc_type_dictionary: u8;
    pub static _xpc_type_connection: u8;
}
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    pub fn SecCodeCreateWithXPCMessage(message: Object, flags: u32, result: *mut Object) -> i32;
    pub fn SecRequirementCreateWithString(
        requirement: Object,
        flags: u32,
        result: *mut Object,
    ) -> i32;
    pub fn SecCodeCopySelf(flags: u32, result: *mut Object) -> i32;
    pub fn SecCodeCheckValidity(code: Object, flags: u32, requirement: Object) -> i32;
    pub fn SecCodeCopySigningInformation(code: Object, flags: u32, result: *mut Object) -> i32;
    pub static kSecCodeInfoTeamIdentifier: Object;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFStringCreateWithCString(
        allocator: Object,
        text: *const c_char,
        encoding: u32,
    ) -> Object;
    pub fn CFRelease(object: Object);
    pub fn CFDictionaryGetValue(dictionary: Object, key: Object) -> Object;
    pub fn CFStringGetCString(
        string: Object,
        buffer: *mut c_char,
        capacity: isize,
        encoding: u32,
    ) -> u8;
}

/// XPC objects are thread-safe, reference-counted objects. Each owned wrapper holds one retain.
pub struct Xpc(pub Object);
unsafe impl Send for Xpc {}
unsafe impl Sync for Xpc {}
impl Clone for Xpc {
    fn clone(&self) -> Self {
        Self(unsafe { xpc_retain(self.0) })
    }
}
impl Drop for Xpc {
    fn drop(&mut self) {
        unsafe { xpc_release(self.0) }
    }
}
impl Xpc {
    pub unsafe fn retain(object: Object) -> Self {
        Self(unsafe { xpc_retain(object) })
    }
}
pub struct Connection(pub Xpc);
impl Drop for Connection {
    fn drop(&mut self) {
        unsafe { xpc_connection_cancel(self.0.0) }
    }
}
