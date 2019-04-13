use crate::mount::MOUNTS_FILE;
use failure::Error;
use nix::errno::Errno;
use nix::sys::signal::{kill, SIGTERM};
use nix::unistd::{getuid, Pid};
use nix::Error as SyscallError;
use std::fs::{create_dir, read_dir, read_to_string, remove_dir, write, DirEntry};
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Fail)]
enum CgroupError {
    #[fail(display = "cgroup2 not mounted")]
    CgroupNotMounted,
}

const CGROUP_PROCS: &str = "cgroup.procs";
const CGROUP_FS: &str = "cgroup2";

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

fn get_ruthless_cgroup_path() -> Result<PathBuf, Error> {
    let cgroup_path = find_cgroups_path()?.ok_or(CgroupError::CgroupNotMounted)?;
    let ruthless_cgroup = Path::new(&cgroup_path).join(ruthless_cgroup_path());
    Ok(ruthless_cgroup)
}

pub(crate) fn get_active_cgroups() -> Result<Vec<String>, Error> {
    let containers_location = get_ruthless_cgroup_path()?;
    let cgroup_content: Vec<DirEntry> =
        read_dir(containers_location)?.collect::<Result<Vec<DirEntry>, _>>()?;
    let mut result = Vec::new();
    for entry in cgroup_content {
        if entry.file_type()?.is_dir() {
            result.push(
                entry
                    .file_name()
                    .to_str()
                    .unwrap()
                    .to_owned()
                    .replace("-core", ""),
            )
        }
    }
    Ok(result)
}

pub(crate) fn terminate_cgroup_processes(container_name: &str) -> Result<(), Error> {
    let containers_location = get_ruthless_cgroup_path()?;
    let container_location = containers_location.join(format!(
        "{}-core/{}-processes",
        container_name, container_name
    ));
    let pids: Vec<i32> = read_to_string(container_location.join("cgroup.procs"))?
        .trim()
        .split('\n')
        .map(|p| i32::from_str(p))
        .collect::<Result<Vec<i32>, _>>()?;
    for p in pids {
        match kill(Pid::from_raw(p), SIGTERM) {
            Ok(()) => {}
            Err(SyscallError::Sys(Errno::ESRCH)) => {}
            Err(e) => Err(e)?,
        }
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) enum CgroupOptions {
    CpuWeight(usize),
    CpuWeightNice(isize),
    CpuMax(String, usize),
    CpusetCpus(String),
    CpusetCpusPartition(String),
    CpusetMems(String),
    IoMax(String),
    IoWeight(String, usize),
    MemoryHigh(String),
    MemoryLow(usize),
    MemoryMax(String),
    MemoryMin(usize),
    MemoryOomGroup(usize),
    MemorySwapMax(String),
    PidsMax(usize),
    RdmaMax(String),
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
                CgroupOptions::CpuMax(max, period) => {
                    cgroup.set_cpu_max(max.as_str(), *period as isize)?;
                }
                CgroupOptions::CpuWeight(weight) => {
                    cgroup.set_cpu_weight(weight)?;
                }
                CgroupOptions::CpuWeightNice(weight) => {
                    cgroup.set_cpu_weight_nice(weight)?;
                }
                CgroupOptions::CpusetCpus(cpus) => {
                    cgroup.set_cpuset_cpus(cpus.as_str())?;
                }
                CgroupOptions::CpusetCpusPartition(cpus) => {
                    cgroup.set_cpuset_cpus_partition(cpus.as_str())?;
                }
                CgroupOptions::CpusetMems(mems) => {
                    cgroup.set_cpuset_mems(mems.as_str())?;
                }
                CgroupOptions::IoMax(max) => {
                    cgroup.set_io_max(max.as_str())?;
                }
                CgroupOptions::IoWeight(range, weight) => {
                    cgroup.set_io_weight(range.as_str(), *weight as isize)?;
                }
                CgroupOptions::MemoryHigh(high) => {
                    cgroup.set_memory_high(high.as_str())?;
                }
                CgroupOptions::MemoryLow(low) => {
                    cgroup.set_memory_low(low)?;
                }
                CgroupOptions::MemoryMax(max) => {
                    cgroup.set_memory_max(max.as_str())?;
                }
                CgroupOptions::MemoryMin(min) => {
                    cgroup.set_memory_min(min)?;
                }
                CgroupOptions::MemoryOomGroup(group) => {
                    cgroup.set_memory_oom_group(group)?;
                }
                CgroupOptions::MemorySwapMax(max) => {
                    cgroup.set_memory_swap_max(max.as_str())?;
                }
                CgroupOptions::PidsMax(max) => {
                    cgroup.set_pids_max(max)?;
                }
                CgroupOptions::RdmaMax(max) => {
                    cgroup.set_rdma_max(max.as_str())?;
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

macro_rules! cgroup_controller_interface_string_number {
    ($self: ident, $path: expr, $name: ident) => {
        fn $name(&$self, value1: &str, value2: isize) -> Result<(), Error> {
            write($self.parent.join($path), format!("{} {}", value1, value2))?;
            Ok(())
        }
    }
}

macro_rules! cgroup_controller_interface {
    ($self: ident, $arg_type: ident, $path: expr, $name: ident) => {
        fn $name(&$self, value: &$arg_type) -> Result<(), Error> {
            write($self.parent.join($path), format!("{}", value))?;
            Ok(())
        }
    }
}

impl Cgroup {
    fn new(name: &str) -> Result<Cgroup, Error> {
        let ruthless_cgroup = get_ruthless_cgroup_path()?;
        let parent_name = format!("{}-core", name);
        let cgroup_name = format!("{}-processes", name);
        let parent = ruthless_cgroup.join(parent_name);
        let path = parent.join(&cgroup_name);

        create_dir(&parent)?;
        create_dir(&path)?;

        Ok(Cgroup { parent, path })
    }

    cgroup_controller_interface!(self, usize, "cpu.weight", set_cpu_weight);
    cgroup_controller_interface!(self, isize, "cpu.weight.nice", set_cpu_weight_nice);
    cgroup_controller_interface_string_number!(self, "cpu.max", set_cpu_max);
    cgroup_controller_interface!(self, str, "cpuset.cpus", set_cpuset_cpus);
    cgroup_controller_interface!(
        self,
        str,
        "cpuset.cpus.partition",
        set_cpuset_cpus_partition
    );
    cgroup_controller_interface!(self, str, "cpuset.mems", set_cpuset_mems);
    cgroup_controller_interface!(self, str, "io.max", set_io_max);
    cgroup_controller_interface_string_number!(self, "io.weight", set_io_weight);
    cgroup_controller_interface!(self, str, "memory.high", set_memory_high);
    cgroup_controller_interface!(self, usize, "memory.low", set_memory_low);
    cgroup_controller_interface!(self, str, "memory.max", set_memory_max);
    cgroup_controller_interface!(self, usize, "memory.min", set_memory_min);
    cgroup_controller_interface!(self, usize, "memory.oom.group", set_memory_oom_group);
    cgroup_controller_interface!(self, str, "memory.swap.max", set_memory_swap_max);
    cgroup_controller_interface!(self, usize, "pids.max", set_pids_max);
    cgroup_controller_interface!(self, str, "rdma.max", set_rdma_max);

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
