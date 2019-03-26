use crate::mount::MOUNTS_FILE;
use failure::Error;
use std::env::home_dir;
use std::path::PathBuf;

#[derive(Debug, Fail)]
pub(crate) enum ImageError {
    #[fail(display="No home directory")]
    NoHomeDirectory,
}

fn get_image_repository_path() -> Result<PathBuf, Error> {
    let home_path = home_dir().ok_or(ImageError::NoHomeDirectory)?;
    Ok(home_path.join(".local/lib/ruthless"))
}