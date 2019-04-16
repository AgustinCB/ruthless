use crate::jaillogs::LOGS_PATH;
use crate::mount::MOUNTS_FILE;
use dirs::home_dir;
use failure::Error;
use nix::{Error as SyscallError};
use nix::dir::{Dir, Type};
use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;
use std::fs::{metadata, read, read_dir, remove_dir_all, remove_file, File};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use tar::{Archive, Entry};

pub(crate) const BTRFS_IOCTL_MAGIC: u64 = 0x94;
pub(crate) const BTRFS_IOC_GET_SUBVOL_INFO: u64 = 60;
pub(crate) const BTRFS_IOC_SNAP_CREATE: u64 = 1;
pub(crate) const BTRFS_IOC_SUBVOL_CREATE: u64 = 14;
pub(crate) const BTRFS_IOC_SNAP_DESTROY: u64 = 15;
const BTRFS_PATH_NAME_MAX: usize = 4087;
const BTRFS_VOL_NAME_MAX: usize = 255;
const BTRFS_UUID_SIZE: usize = 16;
const LIB_LOCATION: &str = ".local/lib/ruthless/images";

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

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BtrfsTimespec {
    sec: u64,
    nsec: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BtrfsSubvolInfo {
    treeid: u64,
    name: [char; BTRFS_VOL_NAME_MAX + 1],
    parent_id: u64,
    dirid: u64,
    generation: u64,
    flags: u64,
    uuid: [u8; BTRFS_UUID_SIZE],
    parent_uuid: [u8; BTRFS_UUID_SIZE],
    received_uuid: [u8; BTRFS_UUID_SIZE],
    ctransid: u64,
    otransid: u64,
    stransid: u64,
    rtransid: u64,
    reserved: [u64; 8],
    ctime: BtrfsTimespec,
    otime: BtrfsTimespec,
    stime: BtrfsTimespec,
    rtime: BtrfsTimespec,
}

impl Default for BtrfsSubvolInfo {
    fn default() -> Self {
        BtrfsSubvolInfo {
            treeid: u64::default(),
            name: [char::default(); BTRFS_VOL_NAME_MAX + 1],
            parent_id: u64::default(),
            dirid: u64::default(),
            generation: u64::default(),
            flags: u64::default(),
            uuid: [u8::default(); BTRFS_UUID_SIZE],
            parent_uuid: [u8::default(); BTRFS_UUID_SIZE],
            received_uuid: [u8::default(); BTRFS_UUID_SIZE],
            ctransid: u64::default(),
            otransid: u64::default(),
            stransid: u64::default(),
            rtransid: u64::default(),
            reserved: [u64::default(); 8],
            ctime: BtrfsTimespec::default(),
            otime: BtrfsTimespec::default(),
            stime: BtrfsTimespec::default(),
            rtime: BtrfsTimespec::default(),
        }
    }
}

ioctl_write_ptr!(
    btrfs_ioc_snap_create,
    BTRFS_IOCTL_MAGIC,
    BTRFS_IOC_SNAP_CREATE,
    BtrfsVolArgs
);
ioctl_write_ptr!(
    btrfs_ioc_subvol_create,
    BTRFS_IOCTL_MAGIC,
    BTRFS_IOC_SUBVOL_CREATE,
    BtrfsVolArgs
);
ioctl_write_ptr!(
    btrfs_ioc_snap_delete,
    BTRFS_IOCTL_MAGIC,
    BTRFS_IOC_SNAP_DESTROY,
    BtrfsVolArgs
);
ioctl_read!(
    btrfs_ioc_get_subvol_info,
    BTRFS_IOCTL_MAGIC,
    BTRFS_IOC_GET_SUBVOL_INFO,
    BtrfsSubvolInfo
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
    #[fail(display = "No name path {:?}", 0)]
    NoNamePath(PathBuf),
    #[fail(display = "No parent path {:?}", 0)]
    NoParentPath(PathBuf),
    #[fail(display = "Can't convert OsString {:?}", 0)]
    OsStringConversionError(PathBuf),
}

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

#[inline]
fn path_to_file_name_str(path: &Path) -> Result<&str, Error> {
    Ok(path
        .file_name()
        .ok_or_else(|| ImageError::NoNamePath(path.to_path_buf()))?
        .to_str()
        .ok_or_else(|| ImageError::OsStringConversionError(path.to_path_buf()))?)
}

type OCIQueues<'a> = (
    Vec<Entry<'a, File>>,
    Vec<Entry<'a, File>>,
    Vec<Entry<'a, File>>,
);

fn create_steps_queues(layer_tar_file: &mut Archive<File>) -> Result<OCIQueues, Error> {
    let mut opaque_whiteouts = Vec::new();
    let mut whiteouts = Vec::new();
    let mut modifications = Vec::new();
    for entry_result in layer_tar_file.entries()? {
        let entry = entry_result?;
        let path = entry.path()?;
        let entry_name = path_to_file_name_str(&path)?;
        if entry_name == ".wh..wh..opq" {
            opaque_whiteouts.push(entry);
        } else if entry_name.starts_with(".wh.") {
            whiteouts.push(entry);
        } else {
            modifications.push(entry);
        }
    }
    Ok((opaque_whiteouts, whiteouts, modifications))
}

fn apply_modifications(
    modifications: &mut Vec<Entry<File>>,
    snapshot_path: &PathBuf,
) -> Result<(), Error> {
    for modification in modifications {
        let path = snapshot_path.join(modification.path()?);
        modification.unpack(path)?;
    }
    Ok(())
}

fn apply_whiteouts(whiteouts: &mut Vec<Entry<File>>, snapshot_path: &PathBuf) -> Result<(), Error> {
    for whiteout in whiteouts {
        let original_path = whiteout.path()?;
        let file_name = path_to_file_name_str(&original_path)?;
        let path = snapshot_path.join(original_path.with_file_name(file_name.replace(".wh.", "")));
        if path.is_dir() {
            remove_dir_all(path)?;
        } else {
            remove_file(path)?;
        }
    }
    Ok(())
}

fn apply_opaque_whiteouts(
    opaque_whiteouts: &mut Vec<Entry<File>>,
    snapshot_path: &PathBuf,
) -> Result<(), Error> {
    for opaque_whiteout in opaque_whiteouts {
        let original_path = opaque_whiteout.path()?;
        let dir = snapshot_path.join(
            original_path
                .parent()
                .ok_or_else(|| ImageError::NoParentPath(original_path.to_path_buf()))?,
        );
        for entry in read_dir(dir)? {
            remove_file(entry?.path())?;
        }
    }
    Ok(())
}

fn from_layer_to_snapshot(
    layer_tar_file: &mut Archive<File>,
    snapshot_path: &PathBuf,
) -> Result<(), Error> {
    let (mut opaque_whiteouts, mut whiteouts, mut modifications) =
        create_steps_queues(layer_tar_file)?;
    apply_opaque_whiteouts(&mut opaque_whiteouts, snapshot_path)?;
    apply_whiteouts(&mut whiteouts, snapshot_path)?;
    apply_modifications(&mut modifications, snapshot_path)?;
    Ok(())
}

pub(crate) struct ImageRepository {
    pub path: PathBuf,
}

impl ImageRepository {
    pub(crate) fn new() -> Result<ImageRepository, Error> {
        let path = get_image_repository_path()?;
        Ok(ImageRepository { path })
    }

    pub(crate) fn get_logs_path(&self, container: &str) -> PathBuf {
        self.path.join(container).join(LOGS_PATH[1..].to_owned())
    }

    pub(crate) fn get_image_location_for_process(
        &self,
        image: &str,
        name: &str,
    ) -> Result<PathBuf, Error> {
        let file = metadata(image);
        match file {
            Ok(m) => {
                let path = PathBuf::from(image);
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
            if let Some(Type::Directory) = entry.file_type() {
                let name = entry.file_name().to_str()?;
                if name != "." && name != ".." {
                    result.push(name.to_owned())
                }
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

    pub(crate) fn create_image_from_path(&self, name: &str, path: &PathBuf) -> Result<(), Error> {
        let subvolume_path = self.create_image_subvolume(name)?;
        let mut layer_content = Archive::new(File::open(path)?);
        layer_content.unpack(subvolume_path)?;
        Ok(())
    }

    pub(crate) fn create_layer_for_image(
        &self,
        name: &str,
        parent: &str,
        layer_path: &PathBuf,
    ) -> Result<(), Error> {
        let parent_path = self.path.join(parent);
        let path = self.create_image_snapshot(&parent_path, name)?;
        let mut tar_file = Archive::new(File::open(layer_path)?);
        from_layer_to_snapshot(&mut tar_file, &path)?;
        Ok(())
    }

    pub(crate) fn get_image_info(&self, name: &str) -> Result<Option<BtrfsSubvolInfo>, Error> {
        let mut result = BtrfsSubvolInfo::default();
        let path = Dir::open(&self.path.join(name), OFlag::O_DIRECTORY, Mode::S_IRWXU)?;
        let info_result =
            unsafe { btrfs_ioc_get_subvol_info(path.as_raw_fd() as i32, &mut result as *mut _) };
        match info_result {
            Ok(_) => Ok(Some(result)),
            Err(SyscallError::Sys(Errno::ENOENT)) => Ok(None),
            Err(e) => Err(e)?,
        }
    }

    fn create_image_snapshot(&self, parent: &PathBuf, name: &str) -> Result<PathBuf, Error> {
        let repository = Dir::open(&self.path, OFlag::O_DIRECTORY, Mode::S_IRWXU)?;
        let source = Dir::open(parent, OFlag::O_DIRECTORY, Mode::S_IRWXU)?;
        let args = BtrfsVolArgs::new(i64::from(source.as_raw_fd()), name);
        unsafe { btrfs_ioc_snap_create(repository.as_raw_fd() as i32, &args) }?;
        Ok(self.path.join(name))
    }

    fn create_image_subvolume(&self, name: &str) -> Result<PathBuf, Error> {
        let repository = Dir::open(&self.path, OFlag::O_DIRECTORY, Mode::S_IRWXU)?;
        let args = BtrfsVolArgs::new(0i64, name);
        unsafe { btrfs_ioc_subvol_create(repository.as_raw_fd() as i32, &args) }?;
        Ok(self.path.join(name))
    }
}
