use failure::Error;
use std::fs::{create_dir, read, remove_dir, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Fail)]
enum CgroupError {
    #[fail(display="Process doesn't have cgroup")]
    NoCgroup,
    #[fail(display="cgroup2 not mounted")]
    CgroupNotMounted,
}

const CGROUP_FILE: &'static str = "/proc/self/cgroup";
const MOUNTS_FILE: &'static str = "/proc/mounts";

fn get_unix_timestamp() -> Result<u64, Error> {
    Ok(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs()
    )
}

fn find_process_cgroup() -> Result<Option<String>, Error> {
    let cgroup_bytes = read(CGROUP_FILE)
        .map_err(|e| Error::from(e))?;
    let cgroup_content = String::from_utf8(cgroup_bytes)?;
    Ok(cgroup_content
        .split('\n')
        .find(|s| s.starts_with("0::"))
        .map(|s| s.replace("0::/", "").to_owned())
    )
}

fn find_cgroups_path() -> Result<Option<String>, Error> {
    let mounts_bytes = read(MOUNTS_FILE)
        .map_err(|e| Error::from(e))?;
    let mounts_content = String::from_utf8(mounts_bytes)?;
    Ok(
        mounts_content.split('\n')
            .find(|s| s.starts_with("cgroup2"))
            .map(|s| s.split(' ').collect::<Vec<&str>>()[1].to_owned())
    )
}

pub(crate) struct Cgroup {
    path: PathBuf,
}

impl Cgroup {
    pub(crate) fn new() -> Result<Cgroup, Error> {
        let cgroup_path = find_cgroups_path()?.ok_or(Error::from(CgroupError::CgroupNotMounted))?;
        let cgroup = find_process_cgroup()?.ok_or(Error::from(CgroupError::NoCgroup))?;
        let new_cgroup_name = format!("ruthless-{}", get_unix_timestamp()?);
        let path = Path::new(&cgroup_path).join(cgroup).join(&new_cgroup_name);

        create_dir(path.clone())?;

        Ok(Cgroup {
            path
        })
    }

    pub(crate) fn set_max_processes(&self, max_pids: usize) -> Result<(), Error> {
        write(
            self.path.join("pids.max"), format!("{}", max_pids),
        )
            .map(|_| ())
            .map_err(|e| Error::from(e))
    }

    pub(crate) fn add_pid(&self, pid: u32) -> Result<(), Error> {
        write(
            self.path.join("cgroup.procs"), format!("{}", pid),
        )
            .map(|_| ())
            .map_err(|e| Error::from(e))
    }
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        remove_dir(self.path.clone()).unwrap();
    }
}