use failure::Error;
use std::fs::{create_dir, File};
use std::path::{Path, PathBuf};

pub(crate) const LOGS_PATH: &str = "/tmp/ruthless";

pub(crate) struct JailLogs {
    folder: PathBuf,
}

impl JailLogs {
    pub(crate) fn new() -> Result<JailLogs, Error> {
        let folder = Path::new(LOGS_PATH).to_path_buf();
        create_dir(&folder)?;
        Ok(JailLogs { folder })
    }
    pub(crate) fn stdin(&self) -> Result<File, Error> {
        Ok(File::create(self.folder.join("stdin"))?)
    }
    pub(crate) fn stdout(&self) -> Result<File, Error> {
        Ok(File::create(self.folder.join("stdout"))?)
    }
    pub(crate) fn stderr(&self) -> Result<File, Error> {
        Ok(File::create(self.folder.join("stderr"))?)
    }
}
