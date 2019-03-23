use failure::Error;
use libc::{c_int, __errno_location, mount, umount};
use std::ffi::CString;

#[derive(Debug, Fail)]
enum MountError {
    #[fail(display="Error mounting endpoint: {}", errno)]
    MountingError { errno: c_int },
    #[fail(display="Error umounting endpoint: {}", errno)]
    UmountingError { errno: c_int },
}

pub(crate) struct Mount {
    target: String,
}

impl Mount {
    pub(crate) fn new(resource: String, target: String, fs_type: String) -> Result<Mount, Error> {
        let res = unsafe {
            mount(
                str_to_pointer!(resource),
                str_to_pointer!(target),
                str_to_pointer!(fs_type),
                0,
                std::ptr::null(),
            )
        };
        if res != 0 {
            Err(Error::from(MountError::MountingError { errno: unsafe { *__errno_location() } }))
        } else {
            Ok(Mount { target })
        }
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        let res = unsafe {
            umount(str_to_pointer!(self.target))
        };
        if res != 0 {
            panic!("{}", MountError::UmountingError { errno: unsafe { *__errno_location() } })
        }
    }
}
