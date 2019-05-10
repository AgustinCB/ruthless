use crate::btrfs_send::{BtrfsSend, BtrfsSendCommand, Timespec};
use crate::images::{btrfs_ioc_send, BtrfsSendArgs, BtrfsSubvolInfo, ImageRepository};
use chrono::prelude::Utc;
use failure::Error;
use nix::libc::{chmod, gid_t, link, lremovexattr, lsetxattr, rmdir, uid_t};
use nix::dir::Dir;
use nix::errno::{Errno, errno};
use nix::fcntl::OFlag;
use nix::sys::stat::{Mode, SFlag, mknod, mode_t, utimensat, UtimensatFlags};
use nix::unistd::{pipe, read, mkdir, mkfifo, symlinkat, unlink, truncate, chown, Uid, Gid};
use nix::Error as SyscallError;
use ring::digest::{SHA256, digest};
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs::{read_to_string, File, read_dir, copy, rename};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tar::{Archive, Builder};
use tempdir::TempDir;
use std::io::{Write, Read};
use std::os::raw::c_void;
use std::env::consts::ARCH;
use uuid::Uuid;

const OCI_IMAGE_TEMP: &str = "ruthless_oci_image";
const OCI_IMAGE_REPOSITORIES_PATH: &str = "repositories";

#[derive(Deserialize, Serialize)]
struct Config {
    #[serde(rename = "Hostname")]
    hostname: String,
}

#[derive(Deserialize, Serialize)]
struct LayerJson {
    architecture: String,
    config: Config,
    created: String,
    id: String,
    os: String,
    docker_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
}

#[derive(Deserialize, Clone)]
struct OCIImageRepositoriesFileLatest {
    latest: String,
}
type OCIImageRepositoriesFile = HashMap<String, OCIImageRepositoriesFileLatest>;

pub(crate) struct OCIImage {
    tar_content: TempDir,
    name: String,
}

#[derive(Debug, Fail)]
enum OCIImageError {
    #[fail(display = "No layer name")]
    LayerHasNoName,
    #[fail(display = "The image has no layers")]
    NoLayers,
    #[fail(display = "Tar path has no file name.")]
    NoFileName,
    #[fail(display = "Repositories file doesn't have key {} between {:?}", 0, 1)]
    RepositoryFileIncomplete(String, Vec<String>),
    #[fail(display = "Invalid mode {}", 0)]
    InvalidMode(mode_t),
    #[fail(display = "Syscall error {}", 0)]
    SyscallError(i32),
}

#[inline]
fn layer_name<'a>(
    container_name: &'a str,
    layer: &'a PathBuf,
    pending_layers: &'a [String],
) -> Result<&'a str, Error> {
    if pending_layers.is_empty() {
        Ok(container_name)
    } else {
        Ok(layer
            .file_name()
            .ok_or(OCIImageError::LayerHasNoName)?
            .to_str()
            .ok_or(OCIImageError::NoFileName)?)
    }
}

#[inline]
fn recover_from_eexist(result: Result<(), Error>) -> Result<(), Error> {
    if let Err(e) = result {
        let failure: Option<&SyscallError> = e.downcast_ref();
        if let Some(SyscallError::Sys(Errno::EEXIST)) = failure {
            Ok(())
        } else {
            Err(e)
        }
    } else {
        Ok(())
    }
}

fn get_btrfs_send(
    image_repository: &ImageRepository,
    info: &BtrfsSubvolInfo,
) -> Result<BtrfsSend, Error> {
    let (read_end, write_end) = pipe()?;
    let clone_sources = vec![info.parent_id];
    let args = BtrfsSendArgs {
        fd: i64::from(write_end),
        clone_sources_count: 1,
        clone_sources: &clone_sources,
        parent_root: info.parent_id,
        flags: 0,
        reserved: [0; 4],
    };
    let name = info.name.to_vec().iter().collect::<String>();
    let subvol_fd = Dir::open(
        &image_repository.path.join(name.as_str()),
        OFlag::O_DIRECTORY,
        Mode::S_IRWXU,
    )?;
    unsafe { btrfs_ioc_send(subvol_fd.as_raw_fd(), &args) }?;
    let mut content = Vec::new();
    let mut read_cache = [0; 1024];
    while read(read_end, &mut read_cache)? != 0 {
        content.extend(read_cache.iter());
        read_cache = [0; 1024];
    }
    Ok(BtrfsSend::try_from(content)?)
}

#[inline]
fn get_btrfs_subvolume_stack(
    image_repository: &ImageRepository,
    name: &str,
) -> Result<Vec<BtrfsSubvolInfo>, Error> {
    let mut stack = Vec::new();
    let mut current_name = name.to_owned();
    while let Some(i) = image_repository.get_image_info(current_name.as_str())? {
        stack.push(i);
        current_name = i
            .parent_uuid
            .to_vec()
            .iter()
            .map(|u| *u as char)
            .collect::<String>();
    }
    Ok(stack)
}

fn path_to_c_pointer(path: &PathBuf) -> *const i8 {
    let chars: Vec<i8> = path.as_os_str()
        .to_string_lossy()
        .as_bytes()
        .iter()
        .map(|v| *v as i8)
        .collect();
    chars.as_ptr()
}

fn safe_chmod(path: &PathBuf, mode: mode_t) -> Result<(), Error> {
    let res = unsafe {
        chmod(path_to_c_pointer(path), mode)
    };
    if res == 0 {
        Ok(())
    } else {
        let err = errno();
        Err(OCIImageError::SyscallError(err))?
    }
}

fn safe_lremovexattr(path: &PathBuf, name: &str) -> Result<(), Error> {
    let res = unsafe {
        lremovexattr(
            path_to_c_pointer(path),
            name.as_bytes().iter().map(|v| *v as i8).collect::<Vec<i8>>().as_ptr(),
        )
    };
    if res == 0 {
        Ok(())
    } else {
        let err = errno();
        Err(OCIImageError::SyscallError(err))?
    }
}

fn safe_lsetxattr(path: &PathBuf, name: &str, data: &[u8]) -> Result<(), Error> {
    let res = unsafe {
        lsetxattr(
            path_to_c_pointer(path),
            name.as_bytes().iter().map(|v| *v as i8).collect::<Vec<i8>>().as_ptr(),
            data.as_ptr() as *const c_void,
            data.len(),
            0
        )
    };
    if res == 0 {
        Ok(())
    } else {
        let err = errno();
        Err(OCIImageError::SyscallError(err))?
    }
}

fn safe_link(from: &PathBuf, to: &PathBuf) -> Result<(), Error> {
    let res = unsafe { link(path_to_c_pointer(from), path_to_c_pointer(to)) };
    if res == 0 {
        Ok(())
    } else {
        let err = errno();
        Err(OCIImageError::SyscallError(err))?
    }
}

fn safe_rmdir(path: &PathBuf) -> Result<(), Error> {
    let res = unsafe { rmdir(path_to_c_pointer(path)) };
    if res == 0 {
        Ok(())
    } else {
        let err = errno();
        Err(OCIImageError::SyscallError(err))?
    }
}

fn process_utimes(
    local_path: &PathBuf,
    work_bench: &PathBuf,
    at: Timespec,
    mt: Timespec,
) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    utimensat(None, &full_path, &at.into(), &mt.into(), UtimensatFlags::NoFollowSymlink)?;
    Ok(())
}

fn process_chown(
    local_path: &PathBuf,
    work_bench: &PathBuf,
    uid: u64,
    gid: u64,
) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    chown(&full_path, Some(Uid::from_raw(uid as uid_t)), Some(Gid::from_raw(gid as gid_t)))?;
    Ok(())
}

fn process_chmod(
    local_path: &PathBuf,
    work_bench: &PathBuf,
    mode: u64,
) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    safe_chmod(&full_path, mode as mode_t)
}

fn process_truncate(
    local_path: &PathBuf,
    work_bench: &PathBuf,
    size: u64,
) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    truncate(&full_path, size as i64)?;
    Ok(())
}

fn process_set_xattr(
    local_path: &PathBuf,
    work_bench: &PathBuf,
    xattr: &str,
    data: &[u8],
) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    safe_lsetxattr(&full_path, xattr, &data)?;
    Ok(())
}

fn process_remove_xattr(
    local_path: &PathBuf,
    work_bench: &PathBuf,
    xattr: &str,
) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    safe_lremovexattr(&full_path, xattr)?;
    Ok(())
}

fn process_write(local_path: &PathBuf, work_bench: &PathBuf, data: &[u8]) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    let mut file = File::open(&full_path)?;
    file.write(&data)?;
    Ok(())
}

fn process_rmdir(local_path: &PathBuf, work_bench: &PathBuf) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    safe_rmdir(&full_path)
}

fn process_unlink(local_path: &PathBuf, work_bench: &PathBuf) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    unlink(&full_path)?;
    Ok(())
}

fn process_link(local_path: &PathBuf, to_path: &PathBuf, work_bench: &PathBuf) -> Result<(), Error> {
    let full_to_path = work_bench.join(to_path);
    let full_from_path = work_bench.join(local_path);
    safe_link(&full_from_path, &full_to_path)
}

fn process_rename(from: &PathBuf, to: &PathBuf, work_bench: &PathBuf) -> Result<(), Error> {
    let full_to_path = work_bench.join(to);
    let full_from_path = work_bench.join(from);
    rename(full_from_path, full_to_path)?;
    Ok(())
}

fn process_symlink(local_path: &PathBuf, to_path: &PathBuf, work_bench: &PathBuf) -> Result<(), Error> {
    let full_to_path = work_bench.join(to_path);
    let dir = Dir::open(work_bench, OFlag::empty(), Mode::empty())?;
    symlinkat(&full_to_path, Some(dir.as_raw_fd()), local_path)?;
    Ok(())
}

fn process_mksock(local_path: &PathBuf, work_bench: &PathBuf) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    mknod(&full_path, SFlag::S_IFSOCK,Mode::from_bits(0o600 as mode_t).unwrap(), 0)?;
    Ok(())
}

fn process_mkfifo(local_path: &PathBuf, work_bench: &PathBuf) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    mkfifo(&full_path, Mode::from_bits(0o600 as mode_t).unwrap())?;
    Ok(())
}

fn process_mknode(
    local_path: &PathBuf,
    work_bench: &PathBuf,
    mode: u64,
    dev_t: u64,
) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    mknod(
        &full_path,
        SFlag::S_IFMT,
        Mode::from_bits(mode as mode_t)
            .ok_or_else(|| OCIImageError::InvalidMode(mode as mode_t))?,
        dev_t
    )?;
    Ok(())
}

fn process_mkdir(local_path: &PathBuf, work_bench: &PathBuf) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    mkdir(&full_path, Mode::S_IRWXU)?;
    Ok(())
}

fn process_mkfile(local_path: &PathBuf, work_bench: &PathBuf) -> Result<(), Error> {
    let full_path = work_bench.join(local_path);
    let file = File::create(full_path)?;
    let mut permissions = file.metadata()?.permissions();
    permissions.set_mode(0o600);
    file.set_permissions(permissions)?;
    Ok(())
}

fn process_command(
    c: &BtrfsSendCommand,
    work_bench: &PathBuf,
    name: &str,
    image_repository: &ImageRepository,
) -> Result<(), Error> {
    match c {
        BtrfsSendCommand::SNAPSHOT(_, _, _, _, _) =>
            process_base_subvolume(work_bench, name, image_repository),
        BtrfsSendCommand::MKFILE(local_path) => process_mkfile(local_path, work_bench),
        BtrfsSendCommand::MKDIR(local_path) => process_mkdir(local_path, work_bench),
        BtrfsSendCommand::MKNOD(local_path, mode, dev_t) =>
            process_mknode(local_path, work_bench, *mode, *dev_t),
        BtrfsSendCommand::MKFIFO(local_path) => process_mkfifo(local_path, work_bench),
        BtrfsSendCommand::MKSOCK(local_path) => process_mksock(local_path, work_bench),
        BtrfsSendCommand::SYMLINK(from, to) => process_symlink(from, to, work_bench),
        BtrfsSendCommand::RENAME(from, to) => process_rename(from, to, work_bench),
        BtrfsSendCommand::LINK(from, to) => process_link(from, to, work_bench),
        BtrfsSendCommand::UNLINK(local_path) => process_unlink(local_path, work_bench),
        BtrfsSendCommand::RMDIR(local_path) => process_rmdir(local_path, work_bench),
        BtrfsSendCommand::SET_XATTR(path, name, data) => process_set_xattr(path, work_bench, name.as_str(), data),
        BtrfsSendCommand::REMOVE_XATTR(local_path, name) => process_remove_xattr(local_path, work_bench, name.as_str()),
        BtrfsSendCommand::WRITE(local_path, _, data) => process_write(local_path, work_bench, data),
        BtrfsSendCommand::CLONE(_, _, _, _, _, _, _) => Ok(()),
        BtrfsSendCommand::TRUNCATE(path, size) => process_truncate(path, work_bench, *size),
        BtrfsSendCommand::CHMOD(path, mode) => process_chmod(path, work_bench, *mode),
        BtrfsSendCommand::CHOWN(path, uid, gid) => process_chown(path, work_bench, *uid, *gid),
        BtrfsSendCommand::UTIMES(path, at, mt, _) =>
            process_utimes(path, work_bench, at.clone(), mt.clone()),
        BtrfsSendCommand::UPDATE_EXTENT(_, _, _) => Ok(()),
        BtrfsSendCommand::SUBVOL(_, _, _) => Ok(()),
        BtrfsSendCommand::END => Ok(()),
    }
}

fn get_architecture() -> &'static str {
    if ARCH == "x86_64" {
        "amd64"
    } else {
        ARCH
    }
}

fn process_snapshot(
    snapshot_work_bench: &PathBuf,
    snapshot: BtrfsSend,
    name: &str,
    image_repository: &ImageRepository,
    parent: Option<String>,
) -> Result<(), Error> {
    for c in snapshot.commands.iter() {
        process_command(c, snapshot_work_bench, name, image_repository)?;
    }
    let mut version_file = File::create(snapshot_work_bench.join("VERSION"))?;
    version_file.write(b"1.0")?;
    Builder::new(File::create(snapshot_work_bench.join("layer.tar"))?)
        .append_dir_all(snapshot_work_bench, ".")?;
    let mut layer_file = File::open(snapshot_work_bench.join("layer.tar"))?;
    let mut content = Vec::new();
    layer_file.read(&mut content)?;
    let id_digest = digest(&SHA256, &content);
    let mut digest = String::from("");
    let created = Utc::now();
    id_digest.as_ref().read_to_string(&mut digest)?;
    let config = Config {
        hostname: Uuid::new_v4().to_string(),
    };
    let json = LayerJson {
        architecture: get_architecture().to_owned(),
        created: format!("{:?}", created),
        id: digest,
        os: "linux".to_owned(),
        docker_version: "18.09.2".to_owned(),
        config,
        parent,
    };
    let mut json_file = File::create(snapshot_work_bench.join("json"))?;
    json_file.write(to_string(&json)?.as_bytes())?;
    Ok(())
}

fn process_base_subvolume(
    snapshot_work_bench: &PathBuf,
    name: &str,
    image_repository: &ImageRepository,
) -> Result<(), Error> {
    let subvolume_path = image_repository.path.join(name);
    for entry_result in read_dir(&subvolume_path)? {
        let entry = entry_result?;
        copy(entry.path(), snapshot_work_bench)?;
    }
    Ok(())
}

fn is_subvolume(cmd: &&BtrfsSendCommand) -> bool {
    if let BtrfsSendCommand::SUBVOL(_, _, _) = cmd {
        true
    } else {
        false
    }
}

fn process_subvolume(
    work_bench: &Path,
    volume: BtrfsSend,
    name: &str,
    image_repository: &ImageRepository,
    parent: Option<String>,
) -> Result<(), Error> {
    let snapshot_work_bench = work_bench.join(name);
    if volume.commands.iter().find(is_subvolume).is_none() {
        process_snapshot(&snapshot_work_bench, volume, name, image_repository, parent)
    } else {
        process_base_subvolume(&snapshot_work_bench, name, image_repository)
    }
}

pub(crate) fn export<P: AsRef<Path>>(
    image_repository: &ImageRepository,
    name: &str,
    tarball: P,
) -> Result<(), Error> {
    let stack: Vec<(Vec<u8>, BtrfsSend)> = get_btrfs_subvolume_stack(image_repository, name)?
        .iter()
        .map(|b| get_btrfs_send(image_repository, b).map(|v| (b.uuid.to_vec(), v)))
        .collect::<Result<Vec<(Vec<u8>, BtrfsSend)>, Error>>()?;
    let work_bench = TempDir::new(OCI_IMAGE_TEMP)?;
    let mut parent = None;
    for (uuid, volume) in stack {
        let uuid = String::from_utf8(uuid)?;
        process_subvolume(
            &work_bench.path(),
            volume,
            uuid.as_str(),
            image_repository,
            parent,
        )?;
        parent = Some(uuid);
    }
    let mut archive_builder = Builder::new(File::open(tarball)?);
    archive_builder.append_dir_all(work_bench.path(), ".")?;
    Ok(())
}

impl OCIImage {
    pub(crate) fn new(tar_file_path: &str) -> Result<OCIImage, Error> {
        let path = PathBuf::from_str(tar_file_path)?;
        let name = path
            .file_name()
            .ok_or(OCIImageError::NoFileName)?
            .to_str()
            .ok_or(OCIImageError::NoFileName)?
            .to_owned()
            .split('.')
            .collect::<Vec<&str>>()[0]
            .to_owned();
        let mut tar_file = Archive::new(File::open(tar_file_path)?);
        let tar_content = TempDir::new(OCI_IMAGE_TEMP)?;
        tar_file.unpack(&tar_content.path())?;
        Ok(OCIImage { tar_content, name })
    }

    pub(crate) fn import(&self, image_repository: &ImageRepository) -> Result<(), Error> {
        let repositories_content = self.extract_repositories_content()?;
        let latest_content = self.extract_latest_content(&repositories_content)?;
        let mut layer_stack = self.build_layer_stack(latest_content.latest.as_str())?;
        self.import_from_layer_stack(image_repository, &mut layer_stack)?;
        Ok(())
    }

    #[inline]
    fn import_from_layer_stack(
        &self,
        image_repository: &ImageRepository,
        layer_stack: &mut Vec<String>,
    ) -> Result<(), Error> {
        let first_layer = self
            .tar_content
            .path()
            .join(layer_stack.pop().ok_or(OCIImageError::NoLayers)?);
        let mut last_layer_processed =
            layer_name(self.name.as_str(), &first_layer, &layer_stack)?.to_owned();
        recover_from_eexist(image_repository.create_image_from_path(
            last_layer_processed.as_str(),
            &first_layer.join("layer.tar"),
        ))?;
        while let Some(layer) = layer_stack.pop() {
            let layer_path = self.tar_content.path().join(layer);
            let new_layer_name = layer_name(self.name.as_str(), &layer_path, layer_stack)?;
            recover_from_eexist(image_repository.create_layer_for_image(
                new_layer_name,
                last_layer_processed.as_str(),
                &layer_path.join("layer.tar"),
            ))?;
            last_layer_processed = new_layer_name.to_owned();
        }
        Ok(())
    }

    #[inline]
    fn build_layer_stack(&self, starting_layer: &str) -> Result<Vec<String>, Error> {
        let mut results = vec![starting_layer.to_owned()];
        let file_path = self.tar_content.path().join(starting_layer);
        let mut layer_json =
            from_str::<LayerJson>(read_to_string(&file_path.join("json"))?.as_str())?;
        while let Some(parent) = layer_json.parent.clone() {
            let next_layer_path = self.tar_content.path().join(parent.as_str());
            results.push(parent);
            layer_json =
                from_str::<LayerJson>(read_to_string(&next_layer_path.join("json"))?.as_str())?;
        }
        Ok(results)
    }

    #[inline]
    fn extract_latest_content<'a>(
        &self,
        repositories_content: &'a OCIImageRepositoriesFile,
    ) -> Result<&'a OCIImageRepositoriesFileLatest, Error> {
        Ok(repositories_content.get(&self.name).ok_or_else(|| {
            OCIImageError::RepositoryFileIncomplete(
                self.name.clone(),
                repositories_content.keys().cloned().collect(),
            )
        })?)
    }

    #[inline]
    fn extract_repositories_content(&self) -> Result<OCIImageRepositoriesFile, Error> {
        Ok(from_str::<OCIImageRepositoriesFile>(
            read_to_string(self.tar_content.path().join(OCI_IMAGE_REPOSITORIES_PATH))?.as_str(),
        )?)
    }
}
