//! Keeps the binary containing this code loaded until the process exits.
//!
//! Network calls that cannot be interrupted run on detached threads, which
//! may outlive the plugin instance. If the host then unloaded the plugin,
//! those threads would run unmapped code and crash the host.

use std::sync::Once;

pub(crate) fn pin_module() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if !imp::pin() {
            log::warn!(target: "whip_send", "cannot pin the plugin binary in memory");
        }
    });
}

#[cfg(unix)]
mod imp {
    pub fn pin() -> bool {
        let addr = pin as fn() -> bool as *const libc::c_void;
        // SAFETY: dladdr only fills `info`; dlopen with RTLD_NOLOAD only
        // touches an image that is already loaded. The handle is never
        // closed, and RTLD_NODELETE keeps the image even if it were.
        unsafe {
            let mut info: libc::Dl_info = std::mem::zeroed();
            if libc::dladdr(addr, &mut info) == 0 || info.dli_fname.is_null() {
                return false;
            }
            let flags = libc::RTLD_NOW | libc::RTLD_NOLOAD | libc::RTLD_NODELETE;
            !libc::dlopen(info.dli_fname, flags).is_null()
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
    };

    pub fn pin() -> bool {
        let addr = pin as fn() -> bool as *const u16;
        let mut module = std::ptr::null_mut();
        // SAFETY: with FROM_ADDRESS the name argument is an address inside
        // this module; PIN keeps the module loaded until the process exits.
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                addr,
                &mut module,
            ) != 0
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn pin() -> bool {
        false
    }
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    #[test]
    fn pins_the_current_binary() {
        assert!(super::imp::pin());
    }
}
