use failure::Error;
use nix::mount::{mount, umount, MsFlags};

pub(crate) const MOUNTS_FILE: &'static str = "/proc/mounts";

pub(crate) struct Mount {
    target: String,
}

impl Mount {
    pub(crate) fn new(resource: String, target: String, fs_type: String) -> Result<Mount, Error> {
        const NONE: Option<&'static [u8]> = None;
        mount(
            Some(resource.as_str()),
            target.as_str(),
            Some(fs_type.as_str()),
            MsFlags::empty(),
            NONE,
        )?;
        Ok(Mount { target })
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        umount(self.target.as_str()).unwrap();
    }
}
