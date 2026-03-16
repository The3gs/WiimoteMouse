use std::ffi::CStr;

#[link(name = "xwiimote")]
unsafe extern "C" {
    fn xwii_monitor_new(poll: bool, direct: bool) -> *const libc::c_void;

    fn xwii_monitor_ref(mon: *const libc::c_void);

    fn xwii_monitor_unref(mon: *const libc::c_void);

    fn xwii_monitor_get_fd(mon: *const libc::c_void, blocking: bool) -> libc::c_int;

    fn xwii_monitor_poll(mon: *const libc::c_void) -> *const i8;
}

pub struct Monitor {
    ptr: *const libc::c_void,
}

impl Monitor {
    pub fn new(poll: bool, direct: bool) -> Self {
        Self {
            ptr: unsafe { xwii_monitor_new(poll, direct) },
        }
    }
}

impl Iterator for Monitor {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        let c_string = unsafe { xwii_monitor_poll(self.ptr) };
        if c_string.is_null() {
            return None;
        } else {
            let string = unsafe { CStr::from_ptr(c_string) }
                .to_string_lossy()
                .into_owned();
            unsafe { libc::free(c_string as *mut libc::c_void) };
            return Some(string);
        }
    }
}

impl Clone for Monitor {
    fn clone(&self) -> Self {
        unsafe {
            xwii_monitor_ref(self.ptr);
        }
        Self { ptr: self.ptr }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        unsafe {
            xwii_monitor_unref(self.ptr);
        }
    }
}
