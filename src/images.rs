use crate::mount::MOUNTS_FILE;
use failure::Error;
use std::env::home_dir;
use std::fs::read;
use std::path::PathBuf;

#[derive(Debug, Fail)]
pub(crate) enum ImageError {
    #[fail(display="No home directory")]
    NoHomeDirectory,
    #[fail(display="Invalid lib path encoding")]
    InvalidLibPathEncoding,
    #[fail(display="Lib path not mounted")]
    LibPathNotMounted,
}

const LIB_LOCATION: &'static str = ".local/lib/ruthless/images";

fn get_image_repository_path() -> Result<PathBuf, Error> {
    let home_path = home_dir().ok_or(ImageError::NoHomeDirectory)?;
    let lib_path = home_path.join(LIB_LOCATION);
    let expected_content = format!(" {} btrfs ", lib_path.to_str().ok_or(ImageError::InvalidLibPathEncoding)?);
    let mounts_bytes = read(MOUNTS_FILE)
        .map_err(|e| Error::from(e))?;
    let mounts_contents = String::from_utf8(mounts_bytes)?;
    if mounts_contents.contains(&expected_content) {
        Ok(lib_path)
    } else {
        Err(ImageError::LibPathNotMounted)?
    }
}