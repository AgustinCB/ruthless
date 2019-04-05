use crate::mount::MOUNTS_FILE;
use failure::Error;
use nix::unistd::getuid;
use std::fs::{create_dir, read_to_string, remove_dir, write};
use std::path::{Path, PathBuf};

#[derive(Debug, Fail)]
enum CgroupError {
    #[fail(display="cgroup2 not mounted")]
    CgroupNotMounted,
}

const CGROUP_PROCS: &'static str = "cgroup.procs";
const PIDS_MAX: &'static str = "pids.max";
const CGROUP_FS: &'static str = "cgroup2";

fn find_cgroups_path() -> Result<Option<String>, Error> {
    let mounts_content = read_to_string(MOUNTS_FILE)?;
    Ok(
        mounts_content.split('\n')
            .find(|s| s.starts_with(CGROUP_FS))
            .map(|s| s.split(' ').collect::<Vec<&str>>()[1].to_owned())
    )
}

#[inline]
fn ruthless_cgroup_path() -> String {
    let uid = getuid();
    format!("user.slice/user-{}.slice/user@{}.service/ruthless", uid, uid)
}

pub(crate) struct Cgroup {
    parent: PathBuf,
    path: PathBuf,
}

impl Cgroup {
    pub(crate) fn new(name: &str) -> Result<Cgroup, Error> {
        let cgroup_path = find_cgroups_path()?.ok_or(CgroupError::CgroupNotMounted)?;
        let ruthless_cgroup = Path::new(&cgroup_path).join(ruthless_cgroup_path());
        let parent_name = format!("{}-core", name);
        let cgroup_name = format!("{}-processes", name);
        let parent = ruthless_cgroup.join(parent_name);
        let path = parent.join(&cgroup_name);

        create_dir(&parent)?;
        create_dir(&path)?;

        Ok(Cgroup {
            parent,
            path
        })
    }

    pub(crate) fn set_max_processes(&self, max_pids: usize) -> Result<(), Error> {
        write(self.parent.join(PIDS_MAX), format!("{}", max_pids))?;
        Ok(())
    }

    pub(crate) fn add_pid(&self, pid: u32) -> Result<(), Error> {
        write(self.path.join(CGROUP_PROCS), format!("{}", pid))?;
        Ok(())
    }
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        remove_dir(self.path.clone()).unwrap();
        remove_dir(self.parent.clone()).unwrap();
    }
}