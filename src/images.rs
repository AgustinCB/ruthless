use crate::mount::MOUNTS_FILE;
use dirs::home_dir;
use failure::Error;
use nix::dir::{Dir, Type};
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;
use std::fs::{metadata, read};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

pub(crate) const BTRFS_IOCTL_MAGIC: u64 = 0x94;
pub(crate) const BTRFS_IOC_SNAP_CREATE: u64 = 1;
pub(crate) const BTRFS_IOC_SNAP_DESTROY: u64 = 15;
const BTRFS_PATH_NAME_MAX: usize = 4087;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BtrfsVolArgs {
    fd: i64,
    name: [u8; BTRFS_PATH_NAME_MAX + 1],
}
impl BtrfsVolArgs {
    pub(crate) fn new(fd: i64, name: &str) -> BtrfsVolArgs {
        let bytes = name.bytes();
        let mut name = [0; BTRFS_PATH_NAME_MAX + 1];
        for (i, b) in bytes.enumerate() {
            if i < name.len() {
                name[i] = b;
            }
        }
        BtrfsVolArgs { fd, name }
    }
}

ioctl_write_ptr!(
    btrfs_ioc_snap_create,
    BTRFS_IOCTL_MAGIC,
    BTRFS_IOC_SNAP_CREATE,
    BtrfsVolArgs
);
ioctl_write_ptr!(
    btrfs_ioc_snap_delete,
    BTRFS_IOCTL_MAGIC,
    BTRFS_IOC_SNAP_DESTROY,
    BtrfsVolArgs
);

#[derive(Debug, Fail)]
pub(crate) enum ImageError {
    #[fail(display = "No home directory")]
    NoHomeDirectory,
    #[fail(display = "Invalid lib path encoding")]
    InvalidLibPathEncoding,
    #[fail(display = "Lib path not mounted")]
    LibPathNotMounted,
    #[fail(display = "Image doesn't exist {:?}", 0)]
    ImageDoesntExist(PathBuf),
    #[fail(display = "Image is not a directory {:?}", 0)]
    ImageIsntDirectory(PathBuf),
}

const LIB_LOCATION: &'static str = ".local/lib/ruthless/images";

fn get_image_repository_path() -> Result<PathBuf, Error> {
    let home_path = home_dir().ok_or(ImageError::NoHomeDirectory)?;
    let lib_path = home_path.join(LIB_LOCATION);
    let expected_content = format!(
        " {} btrfs ",
        lib_path
            .to_str()
            .ok_or(ImageError::InvalidLibPathEncoding)?
    );
    let mounts_bytes = read(MOUNTS_FILE)?;
    let mounts_contents = String::from_utf8(mounts_bytes)?;
    if mounts_contents.contains(&expected_content) {
        Ok(lib_path)
    } else {
        Err(ImageError::LibPathNotMounted)?
    }
}

pub(crate) struct ImageRepository {
    path: PathBuf,
}

impl ImageRepository {
    pub(crate) fn new() -> Result<ImageRepository, Error> {
        let path = get_image_repository_path()?;
        Ok(ImageRepository { path })
    }

    pub(crate) fn get_image_location_for_process(
        &self,
        image: &str,
        name: &str,
    ) -> Result<PathBuf, Error> {
        let file = metadata(image.clone());
        match file {
            Ok(m) => {
                let path = PathBuf::from(image.clone());
                if m.is_dir() {
                    Ok(path)
                } else {
                    Err(ImageError::ImageIsntDirectory(path))?
                }
            }
            Err(_) => {
                let location = self.path.join(image);
                let m = metadata(&location)?;
                if m.is_dir() {
                    Ok(self.create_image_snapshot(&location, name)?)
                } else {
                    Err(ImageError::ImageDoesntExist(location))?
                }
            }
        }
    }

    pub(crate) fn get_images(&self) -> Result<Vec<String>, Error> {
        let mut repository = Dir::open(&self.path, OFlag::O_DIRECTORY, Mode::S_IRUSR)?;
        let mut result = Vec::new();
        for maybe_entry in repository.iter() {
            let entry = maybe_entry?;
            match entry.file_type() {
                Some(Type::Directory) => {
                    let name = entry.file_name().to_str()?;
                    if name != "." && name != ".." {
                        result.push(name.to_owned())
                    }
                }
                _ => {}
            }
        }
        Ok(result)
    }

    pub(crate) fn delete_image(&self, name: &str) -> Result<(), Error> {
        let repository = Dir::open(&self.path, OFlag::O_DIRECTORY, Mode::S_IRWXU)?;
        let args = BtrfsVolArgs::new(-1i64, name);
        unsafe { btrfs_ioc_snap_delete(repository.as_raw_fd() as i32, &args) }?;
        Ok(())
    }

    fn create_image_snapshot(&self, parent: &PathBuf, name: &str) -> Result<PathBuf, Error> {
        let repository = Dir::open(&self.path, OFlag::O_DIRECTORY, Mode::S_IRWXU)?;
        let source = Dir::open(parent, OFlag::O_DIRECTORY, Mode::S_IRWXU)?;
        let args = BtrfsVolArgs::new(source.as_raw_fd() as i64, name);
        unsafe { btrfs_ioc_snap_create(repository.as_raw_fd() as i32, &args) }?;
        Ok(self.path.join(name))
    }
}
