use crate::cgroup::CgroupFactory;
use crate::jaillogs::JailLogs;
use crate::mount::Mount;
use failure::Error;
use nix::sched::{clone, CloneFlags};
use nix::sys::signal::SIGCHLD;
use nix::sys::wait::waitpid;
use nix::unistd::{chroot, getpid, getuid, setuid, Pid, Uid};
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

fn run(run_args: &Vec<String>, redirect_logs: bool) -> Result<isize, Error> {
    let _proc_mount = Mount::new(
        PROC_RESOURCE.to_owned(),
        PROC_TARGET.to_owned(),
        PROC_FS.to_owned(),
    )?;
    let mut command = Command::new(run_args[0].clone());
    command
        .args(run_args[1..].into_iter())
        .env_clear()
        .env(PATH_ENV_VARIABLE, CONTAINER_PATH)
        .current_dir("/");
    if redirect_logs {
        let logs = JailLogs::new()?;
        command
            .stdin(logs.stdin()?)
            .stdout(logs.stdout()?)
            .stderr(logs.stderr()?);
    };
    let exit_status = command
        .spawn()
        .expect(COMMAND_ERROR)
        .wait()?;
    Ok(exit_status.code().unwrap_or(0) as isize)
}

fn start_parent_process(
    args: &Vec<String>,
    image: &str,
    cgroup_factory: &CgroupFactory,
    user_id: Uid,
    redirect_logs: bool,
) -> Result<isize, Error> {
    let mut stack = [0u8; STACK_SIZE];
    let cgroup = cgroup_factory.build()?;
    let pid = clone(
        Box::new(|| {
            cgroup.add_pid(getpid().as_raw() as u32).unwrap();
            set_user_map(user_id).unwrap();
            setuid(Uid::from_raw(0)).unwrap();
            chroot(image.clone()).unwrap();
            run(args, redirect_logs).unwrap()
        }),
        stack.as_mut(),
        CloneFlags::CLONE_NEWNS
            | CloneFlags::CLONE_NEWPID
            | CloneFlags::CLONE_NEWUTS
            | CloneFlags::CLONE_NEWUSER,
        Some(SIGCHLD as i32),
    )?;
    waitpid(pid, None)?;
    Ok(0)
}

pub(crate) struct Jail {
    detach: bool,
    user_id: Uid,
}

impl Jail {
    pub(crate) fn new(detach: bool) -> Jail {
        let user_id = getuid();
        Jail { detach, user_id }
    }

    pub(crate) fn run(
        &mut self,
        args: &Vec<String>,
        image: &str,
        cgroup: &CgroupFactory,
    ) -> Result<(), Error> {
        let pid = self.start_process(args, image, cgroup)?;
        if !self.detach {
            waitpid(pid, None)?;
        }
        Ok(())
    }

    fn start_process(
        &mut self,
        args: &Vec<String>,
        image: &str,
        cgroup: &CgroupFactory,
    ) -> Result<Pid, Error> {
        let mut stack = [0u8; STACK_SIZE];
        let user_id = self.user_id;
        let pid = clone(
            Box::new(|| {
                start_parent_process(args, image, cgroup, user_id, self.detach).unwrap()
            }),
            stack.as_mut(),
            CloneFlags::empty(),
            Some(SIGCHLD as i32),
        )?;
        Ok(pid)
    }
}
