#[macro_use]
extern crate failure;

use failure::Error;
use libc::{c_int, chroot, clone, close, getuid, mount, pipe, read, setuid, umount, waitpid,
           write as cwrite, CLONE_NEWNS, CLONE_NEWPID, CLONE_NEWUTS, CLONE_NEWUSER, SIGCHLD};
use std::env::args;
use std::ffi::{CString, c_void};
use std::fs::write;
use std::os::unix::process::CommandExt;
use std::process::{Command, exit, id};
use std::sync::Arc;

mod cgroup;

use cgroup::Cgroup;

const USAGE: &'static str = "USAGE: ruthless [image] [command]";
const STACK_SIZE: usize = 65536;

#[derive(Debug, Fail)]
enum RunError {
    #[fail(display = "Error creating execution command")]
    ErrorExecutingCommand,
}

struct RunArgs {
    args: Vec<String>,
    image: String,
    pipes: [c_int; 2],
}

impl RunArgs {
    fn new(args: Vec<String>, image: String) -> RunArgs {
        let mut pipes = [0;2];
        unsafe { pipe(pipes.as_mut_ptr()) };
        RunArgs {
            args,
            image,
            pipes,
        }
    }

    fn signal_child(&self) {
        unsafe {
            cwrite(self.pipes[1], vec![0].as_ptr() as *const c_void, 1);
        }
    }

    fn wait_for_parent(&self) {
        unsafe {
            //close(self.pipes[1]);
            read(self.pipes[0], std::ptr::null_mut(), 1);
        };
    }
}

impl Drop for RunArgs {
    fn drop(&mut self) {
        unsafe {
            close(self.pipes[0]);
            close(self.pipes[1]);
        }
    }
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

fn set_user_map(id: c_int) -> Result<(), Error> {
    let user_map_location = format!("/proc/{}/uid_map", id);
    let content = format!("0 {} 1\n", unsafe { getuid() });
    write(user_map_location, content)
        .map(|_| ())
        .map_err(|e| {
            eprintln!("{}", e);
            Error::from(e)
        })
}

fn stack_memory() -> *mut c_void {
    let mut s: Vec<c_void> = Vec::with_capacity(STACK_SIZE);
    unsafe { s.as_mut_ptr().offset(STACK_SIZE as isize) }
}

extern "C" fn run(args: *mut c_void) -> c_int {
    let run_args = unsafe { &mut *(args as *mut RunArgs) };
    run_args.wait_for_parent();
    unsafe { setuid(0) };
    change_root(&run_args.image);
    let _proc_mount = Mount::new(
        "proc".to_owned(),
        "/proc".to_owned(),
        "proc".to_owned(),
    );
    let p_cgroup = Arc::new(Cgroup::new().unwrap());
    let cgroup = p_cgroup.clone();
    Command::new(run_args.args[0].clone())
        .args(run_args.args[1..].into_iter())
        .env_clear()
        .envs(
            vec![("PATH", "/bin:/usr/bin:/usr/local/bin:/sbin:/usr/sbin:/usr/local/sbin")].into_iter()
        )
        .current_dir("/")
        .before_exec(move || {
            cgroup.add_pid(id()).unwrap();
            cgroup.set_max_processes(4).unwrap();
            Ok(())
        })
        .spawn()
        .expect("Command failed to start")
        .wait()
        .unwrap();
    0
}

fn jail(args: Vec<String>, image: String) -> Result<c_int, Error> {
    let stack = stack_memory();
    let mut run_args = RunArgs::new(args, image);
    let c_args: *mut c_void = &mut run_args as *mut _ as *mut c_void;
    let id = unsafe {
        clone(
            run,
            stack,
            CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWUTS | CLONE_NEWUSER | SIGCHLD,
            c_args
        )
    };
    set_user_map(id)?;
    run_args.signal_child();
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

    let child_id = jail(args.collect(), image).unwrap();
    let r = unsafe {
        waitpid(child_id, std::ptr::null_mut(), 0)
    };
    if r < 0 {
        eprintln!("Error on the execution of the command");
        exit(1);
    }
}
