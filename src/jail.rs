use crate::cgroup::Cgroup;
use crate::mount::Mount;
use failure::Error;
use nix::sched::{clone, CloneFlags};
use nix::sys::signal::SIGCHLD;
use nix::sys::wait::waitpid;
use nix::unistd::{chroot, getpid, getuid, setuid, Uid};
use std::fs::write;
use std::process::Command;

const STACK_SIZE: usize = 65536;
const PROC_UID_MAP_FILE: &'static str = "/proc/self/uid_map";
const PATH_ENV_VARIABLE: &'static str = "PATH";
const CONTAINER_PATH: &'static str = "/bin:/usr/bin:/usr/local/bin:/sbin:/usr/sbin:/usr/local/sbin";
const COMMAND_ERROR: &'static str = "Command failed to start";
const PROC_RESOURCE: &'static str = "proc";
const PROC_TARGET: &'static str = "/proc";
const PROC_FS: &'static str = "proc";

fn set_user_map(user_id: Uid) -> Result<(), Error> {
    let content = format!("0 {} 1\n", user_id);
    write(PROC_UID_MAP_FILE, content)?;
    Ok(())
}

fn run(run_args: &Vec<String>) -> Result<isize, Error> {
    let _proc_mount = Mount::new(
        PROC_RESOURCE.to_owned(),
        PROC_TARGET.to_owned(),
        PROC_FS.to_owned(),
    )?;
    let exit_status = Command::new(run_args[0].clone())
        .args(run_args[1..].into_iter())
        .env_clear()
        .env(PATH_ENV_VARIABLE, CONTAINER_PATH)
        .current_dir("/")
        .spawn()
        .expect(COMMAND_ERROR)
        .wait()?;
    Ok(exit_status.code().unwrap() as isize)
}

pub(crate) struct Jail {
    cgroup: Cgroup,
    user_id: Uid,
}

impl Jail {
    pub(crate) fn new(cgroup: Cgroup) -> Jail {
        let user_id = getuid();
        Jail {
            cgroup,
            user_id,
        }
    }

    pub(crate) fn run(&mut self, args: &Vec<String>, image: &str) -> Result<(), Error> {
        let mut stack = [0u8; STACK_SIZE];
        let pid = clone(
            Box::new(|| {
                set_user_map(self.user_id).unwrap();
                setuid(Uid::from_raw(0)).unwrap();
                self.cgroup.add_pid(getpid().as_raw() as u32).unwrap();
                chroot(image).unwrap();
                run(args).unwrap()
            }),
            stack.as_mut(),
            CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWUTS |
                CloneFlags::CLONE_NEWUSER,
            Some(SIGCHLD as i32),
        )?;
        waitpid(pid, None)?;
        Ok(())
    }
}