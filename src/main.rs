#[macro_use]
extern crate failure;

use failure::Error;
use libc::{c_int, chroot, clone, getuid, setuid, waitpid, CLONE_NEWNS, CLONE_NEWPID, CLONE_NEWUTS,
           CLONE_NEWUSER, SIGCHLD, __errno_location};
use std::env::args;
use std::ffi::{CString, c_void};
use std::fs::write;
use std::process::{Command, exit};

macro_rules! str_to_pointer {
    ($str:expr) => {
        CString::from_vec_unchecked($str.as_bytes().to_vec()).as_ptr()
    }
}

macro_rules! get_errorno {
    () => {
        unsafe { *__errno_location() }
    }
}

mod cgroup;
mod images;
mod mount;
mod run_args;

use images::ImageRepository;
use mount::Mount;
use run_args::RunArgs;
use crate::cgroup::Cgroup;

const USAGE: &'static str = "USAGE: ruthless [image] [command]";
const STACK_SIZE: usize = 65536;

#[derive(Debug, Fail)]
enum RunError {
    #[fail(display = "Error creating execution command")]
    ErrorExecutingCommand,
    #[fail(display = "Error changing root directory: {}", errno)]
    ChrootError { errno: c_int },
}

fn change_root(location: &str) -> Result<(), Error> {
    let res = unsafe {
        chroot(str_to_pointer!(location))
    };
    if res == 0 {
        Ok(())
    } else {
        Err(RunError::ChrootError { errno: get_errorno!() })?
    }
}

fn safe_getuid() -> u32 {
    unsafe { getuid() }
}

fn set_user_map(id: c_int) -> Result<(), Error> {
    let user_map_location = format!("/proc/{}/uid_map", id);
    let content = format!("0 {} 1\n", safe_getuid());
    write(user_map_location, content)?;
    Ok(())
}

fn stack_memory() -> *mut c_void {
    let mut s: Vec<c_void> = Vec::with_capacity(STACK_SIZE);
    unsafe { s.as_mut_ptr().offset(STACK_SIZE as isize) }
}

extern "C" fn run(args: *mut c_void) -> c_int {
    let run_args = unsafe { &mut *(args as *mut RunArgs) };
    run_args.wait_for_parent().unwrap();
    unsafe { setuid(0) };
    change_root(&run_args.image).unwrap();
    let _proc_mount = Mount::new(
        "proc".to_owned(),
        "/proc".to_owned(),
        "proc".to_owned(),
    ).unwrap();
    Command::new(run_args.args[0].clone())
        .args(run_args.args[1..].into_iter())
        .env_clear()
        .envs(
            vec![("PATH", "/bin:/usr/bin:/usr/local/bin:/sbin:/usr/sbin:/usr/local/sbin")].into_iter()
        )
        .current_dir("/")
        .spawn()
        .expect("Command failed to start")
        .wait()
        .unwrap();
    0
}

fn jail(args: Vec<String>, image: String) -> Result<(), Error> {
    let stack = stack_memory();
    let cgroup = Cgroup::new()?;
    let mut run_args = RunArgs::new(args, image)?;
    cgroup.set_max_processes(4)?;
    let c_args: *mut c_void = &mut run_args as *mut _ as *mut c_void;
    let id = unsafe {
        clone(
            run,
            stack,
            CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWUTS | CLONE_NEWUSER | SIGCHLD,
            c_args
        )
    };
    if id < 0 {
        Err(RunError::ErrorExecutingCommand)?
    } else {
        set_user_map(id)?;
        cgroup.add_pid(id as u32)?;
        run_args.signal_child()?;
        let r = unsafe {
            waitpid(id, std::ptr::null_mut(), 0)
        };
        if r < 0 {
            Err(RunError::ErrorExecutingCommand)?
        } else {
            Ok(())
        }
    }
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
