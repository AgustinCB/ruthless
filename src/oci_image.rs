use crate::btrfs_send::BtrfsSend;
use crate::images::{btrfs_ioc_send, BtrfsSendArgs, BtrfsSubvolInfo, ImageRepository};
use failure::Error;
use nix::dir::Dir;
use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;
use nix::unistd::{pipe, read};
use nix::Error as SyscallError;
use serde::Deserialize;
use serde_json::from_str;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs::{read_to_string, File};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tar::{Archive, Builder};
use tempdir::TempDir;

const OCI_IMAGE_TEMP: &str = "ruthless_oci_image";
const OCI_IMAGE_REPOSITORIES_PATH: &str = "repositories";

#[derive(Deserialize)]
struct LayerJson {
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
        fd: write_end as i64,
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

fn process_subvolume<P: AsRef<Path>>(_work_bench: P, _volume: BtrfsSend) -> Result<(), Error> {
    Ok(())
}

pub(crate) fn export<P: AsRef<Path>>(
    image_repository: &ImageRepository,
    name: &str,
    tarball: P,
) -> Result<(), Error> {
    let stack: Vec<BtrfsSend> = get_btrfs_subvolume_stack(image_repository, name)?
        .iter()
        .map(|b| get_btrfs_send(image_repository, b))
        .collect::<Result<Vec<BtrfsSend>, Error>>()?;
    let work_bench = TempDir::new(OCI_IMAGE_TEMP)?;
    for volume in stack {
        process_subvolume(&work_bench, volume)?;
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
