use crate::mount::MOUNTS_FILE;
use failure::Error;
use nix::unistd::getuid;
use std::fs::{create_dir, read_to_string, remove_dir, write};
use std::path::{Path, PathBuf};

#[derive(Debug, Fail)]
enum CgroupError {
    #[fail(display = "cgroup2 not mounted")]
    CgroupNotMounted,
}

const CGROUP_PROCS: &'static str = "cgroup.procs";
const CGROUP_FS: &'static str = "cgroup2";

fn find_cgroups_path() -> Result<Option<String>, Error> {
    let mounts_content = read_to_string(MOUNTS_FILE)?;
    Ok(mounts_content
        .split('\n')
        .find(|s| s.starts_with(CGROUP_FS))
        .map(|s| s.split(' ').collect::<Vec<&str>>()[1].to_owned()))
}

#[inline]
fn ruthless_cgroup_path() -> String {
    let uid = getuid();
    format!(
        "user.slice/user-{}.slice/user@{}.service/ruthless",
        uid, uid
    )
}

#[derive(Clone)]
pub(crate) enum CgroupOptions {
    PidsMax(usize),
}

#[derive(Clone)]
pub(crate) struct CgroupFactory {
    name: String,
    options: Vec<CgroupOptions>,
}

impl CgroupFactory {
    pub(crate) fn new(name: String, options: Vec<CgroupOptions>) -> CgroupFactory {
        CgroupFactory { name, options }
    }

    pub(crate) fn build(&self) -> Result<Cgroup, Error> {
        let cgroup = Cgroup::new(self.name.as_str())?;
        for o in self.options.iter() {
            match o {
                CgroupOptions::PidsMax(max) => {
                    cgroup.set_pids_max(max.clone())?;
                }
            }
        }
        Ok(cgroup)
    }
}

pub(crate) struct Cgroup {
    parent: PathBuf,
    path: PathBuf,
}

macro_rules! cgroup_controller_interface{
    ($self: ident, $path: expr, $name: ident) => {
        fn $name(&$self, value: usize) -> Result<(), Error> {
            write($self.parent.join($path), format!("{}", value))?;
            Ok(())
        }
    }
}

impl Cgroup {
    fn new(name: &str) -> Result<Cgroup, Error> {
        let cgroup_path = find_cgroups_path()?.ok_or(CgroupError::CgroupNotMounted)?;
        let ruthless_cgroup = Path::new(&cgroup_path).join(ruthless_cgroup_path());
        let parent_name = format!("{}-core", name);
        let cgroup_name = format!("{}-processes", name);
        let parent = ruthless_cgroup.join(parent_name);
        let path = parent.join(&cgroup_name);

        create_dir(&parent)?;
        create_dir(&path)?;

        Ok(Cgroup { parent, path })
    }

    cgroup_controller_interface!(self, "pids.max", set_pids_max);

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
