//! Minimal PAM client (FFI to libpam) for the lock screen.
//! pam_unix handles both cases: root reads shadow directly; non-root
//! delegates to the SUID unix_chkpwd helper automatically.

#![allow(non_camel_case_types)]

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};

type pam_handle_t = c_void;

#[repr(C)]
struct pam_message {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct pam_response {
    resp: *mut c_char,
    resp_retcode: c_int,
}

#[repr(C)]
struct pam_conv {
    conv: extern "C" fn(
        num_msg: c_int,
        msg: *mut *mut pam_message,
        resp: *mut *mut pam_response,
        appdata: *mut c_void,
    ) -> c_int,
    appdata_ptr: *mut c_void,
}

extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conv: *const pam_conv,
        handle: *mut *mut pam_handle_t,
    ) -> c_int;
    fn pam_authenticate(handle: *mut pam_handle_t, flags: c_int) -> c_int;
    fn pam_end(handle: *mut pam_handle_t, status: c_int) -> c_int;
}

const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_SUCCESS: c_int = 0;

// The password travels to the conversation callback via thread-local
// static: PAM conversations are synchronous within pam_authenticate.
thread_local! {
    static PW: std::cell::RefCell<Option<std::ffi::CString>> =
        const { std::cell::RefCell::new(None) };
}

extern "C" fn conv(
    num_msg: c_int,
    msg: *mut *mut pam_message,
    resp: *mut *mut pam_response,
    _appdata: *mut c_void,
) -> c_int {
    let pw = match PW.with(|p| p.borrow().clone()) {
        Some(p) => p,
        None => return -1, // PAM_CONV_ERR
    };
    let n = num_msg.max(0) as usize;
    let out = unsafe { libc::calloc(n, std::mem::size_of::<pam_response>()) } as *mut pam_response;
    if out.is_null() {
        return -1;
    }
    for i in 0..n {
        let m = unsafe { &**msg.add(i) };
        if m.msg_style == PAM_PROMPT_ECHO_OFF {
            let dup = unsafe { libc::strdup(pw.as_ptr() as *const c_char) };
            unsafe { (*out.add(i)).resp = dup };
        }
    }
    unsafe { *resp = out };
    0
}

/// Authenticate `user` against the named PAM service. True = success.
pub fn authenticate(service: &str, user: &str, password: &str) -> bool {
    let Ok(svc) = CString::new(service) else {
        return false;
    };
    let Ok(usr) = CString::new(user) else {
        return false;
    };
    let Ok(pwc) = CString::new(password) else {
        return false;
    };

    PW.with(|p| *p.borrow_mut() = Some(pwc));

    let conv = pam_conv {
        conv,
        appdata_ptr: std::ptr::null_mut(),
    };
    let mut handle: *mut pam_handle_t = std::ptr::null_mut();
    let rc = unsafe { pam_start(svc.as_ptr(), usr.as_ptr(), &conv, &mut handle) };
    if rc != PAM_SUCCESS || handle.is_null() {
        return false;
    }
    let auth = unsafe { pam_authenticate(handle, 0) };
    eprintln!("[pam] authenticate rc={auth}");
    unsafe { pam_end(handle, auth) };
    auth == PAM_SUCCESS
}
