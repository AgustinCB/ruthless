use crate::mount::MOUNTS_FILE;
use failure::Error;
use std::env::home_dir;
use std::fs::{metadata, read};
use std::path::PathBuf;

#[derive(Debug, Fail)]
pub(crate) enum ImageError {
    #[fail(display="No home directory")]
    NoHomeDirectory,
    #[fail(display="Invalid lib path encoding")]
    InvalidLibPathEncoding,
    #[fail(display="Lib path not mounted")]
    LibPathNotMounted,
    #[fail(display="Image doesn't exist {:?}", 0)]
    ImageDoesntExist(PathBuf),
    #[fail(display="Image is not a directory {:?}", 0)]
    ImageIsntDirectory(PathBuf),
}

const LIB_LOCATION: &'static str = ".local/lib/ruthless/images";

fn get_image_repository_path() -> Result<PathBuf, Error> {
    let home_path = home_dir().ok_or(ImageError::NoHomeDirectory)?;
    let lib_path = home_path.join(LIB_LOCATION);
    let expected_content = format!(" {} btrfs ", lib_path.to_str().ok_or(ImageError::InvalidLibPathEncoding)?);
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

    pub(crate) fn get_image_location(&self, image: &str) -> Result<PathBuf, Error> {
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
                    Ok(location)
                } else {
                    Err(ImageError::ImageDoesntExist(location))?
                }
            }
        }
    }
}