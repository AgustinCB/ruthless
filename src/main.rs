#[macro_use]
extern crate failure;

use failure::Error;
use nix::sched::{CloneFlags, clone};
use nix::unistd::{chroot, getpid, getuid, setuid, Uid};
use nix::sys::wait::waitpid;
use nix::sys::signal::SIGCHLD;
use std::env::args;
use std::fs::write;
use std::process::{Command, exit};

mod cgroup;
mod images;
mod mount;

use images::ImageRepository;
use mount::Mount;
use cgroup::Cgroup;

const USAGE: &'static str = "USAGE: ruthless [image] [command]";

fn set_user_map(user_id: Uid) -> Result<(), Error> {
    let user_map_location = format!("/proc/self/uid_map");
    let content = format!("0 {} 1\n", user_id);
    write(user_map_location, content)?;
    Ok(())
}

fn run(run_args: &Vec<String>) -> Result<isize, Error> {
    let _proc_mount = Mount::new(
        "proc".to_owned(),
        "/proc".to_owned(),
        "proc".to_owned(),
    )?;
    let exit_status = Command::new(run_args[0].clone())
        .args(run_args[1..].into_iter())
        .env_clear()
        .envs(
            vec![("PATH", "/bin:/usr/bin:/usr/local/bin:/sbin:/usr/sbin:/usr/local/sbin")].into_iter()
        )
        .current_dir("/")
        .spawn()
        .expect("Command failed to start")
        .wait()?;
    Ok(exit_status.code().unwrap() as isize)
}

fn jail(args: Vec<String>, image: String) -> Result<(), Error> {
    let cgroup = Cgroup::new()?;
    cgroup.set_max_processes(10)?;
    let mut stack = vec![0u8; 65536];
    let user_id = getuid();
    let pid = clone(
        Box::new(|| {
            set_user_map(user_id).unwrap();
            setuid(Uid::from_raw(0)).unwrap();
            cgroup.add_pid(getpid().as_raw() as u32).unwrap();
            chroot(image.as_str()).unwrap();
            run(&args).unwrap()
        }),
        stack.as_mut(),
        CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWUTS |
            CloneFlags::CLONE_NEWUSER,
        Some(SIGCHLD as i32),
    )?;
    waitpid(pid, None)?;
    Ok(())
}

fn main() {
    let mut args = args();

    if args.len() < 3 {
        eprintln!("{}", USAGE);
        exit(1)
    }

    args.next();
    let image = args.next().unwrap();
    let image_repository = ImageRepository::new().unwrap();
    let image_path = image_repository.get_image_location(&image).unwrap().to_str().unwrap().to_owned();

    jail(args.collect(), image_path).unwrap();
}
