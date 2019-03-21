#[macro_use]
extern crate failure;

use failure::Error;
use libc::{c_int, chroot, clone, mount, umount, waitpid, CLONE_NEWPID, CLONE_NEWUTS, SIGCHLD};
use std::env::args;
use std::ffi::{CString, c_void};
use std::process::{Command, exit};

const USAGE: &'static str = "USAGE: ruthless [image] [command]";
const STACK_SIZE: usize = 65536;

#[derive(Debug, Fail)]
enum RunError {
    #[fail(display = "Error creating execution command")]
    ErrorExecutingCommand,
}

struct RunArgs {
    args: Vec<String>,
}

macro_rules! str_to_pointer {
    ($str:expr) => {
        CString::from_vec_unchecked($str.as_bytes().to_vec()).as_ptr()
    }
}

struct Mount {
    target: String,
}

impl Mount {
    fn new(resource: String, target: String, fs_type: String) -> Mount {
        unsafe {
            mount(
                str_to_pointer!(resource),
                str_to_pointer!(target),
                str_to_pointer!(fs_type),
                0,
                std::ptr::null(),
            )
        };
        Mount { target }
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        unsafe {
            umount(str_to_pointer!(self.target));
        }
    }
}

fn change_root(location: &str) {
    unsafe {
        chroot(str_to_pointer!(location));
    }
}

fn stack_memory() -> *mut c_void {
    let mut s: Vec<c_void> = Vec::with_capacity(STACK_SIZE);
    unsafe { s.as_mut_ptr().offset(STACK_SIZE as isize) }
}

extern "C" fn run(args: *mut c_void) -> c_int {
    let run_args = unsafe { &mut *(args as *mut RunArgs) };
    change_root("/home/agustin/projects/ruthless/root");
    let _proc_mount = Mount::new(
        "proc".to_owned(),
        "/proc".to_owned(),
        "proc".to_owned(),
    );
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

fn jail(args: Vec<String>) -> Result<c_int, Error> {
    let stack = stack_memory();
    let mut run_args = RunArgs { args };
    let c_args: *mut c_void = &mut run_args as *mut _ as *mut c_void;
    let id = unsafe {
        clone(run, stack, CLONE_NEWPID | CLONE_NEWUTS | SIGCHLD, c_args)
    };
    if id < 0 {
        Err(Error::from(RunError::ErrorExecutingCommand))
    } else {
        Ok(id)
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

    let child_id = jail(args.collect()).unwrap();
    let r = unsafe {
        waitpid(child_id, std::ptr::null_mut(), 0)
    };
    if r < 0 {
        eprintln!("Error on the execution of the command");
        exit(1);
    }
}
