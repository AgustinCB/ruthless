use libc::{c_int, clone, wait, CLONE_NEWUTS, SIGCHLD};
use std::env::{args, remove_var, vars};
use std::ffi::c_void;
use std::process::{Command, exit, id};

const USAGE: &'static str = "USAGE: ruthless [image] [command]";
const STACK_SIZE: usize = 65536;

fn clear_environment() {
    for (k, _) in vars() {
        remove_var(k);
    }
}

fn stack_memory() -> Vec<u8> {
    Vec::with_capacity(STACK_SIZE)
}

fn run(program: String, args: Vec<String>) {
    clear_environment();
    Command::new(program)
        .args(args)
        .spawn()
        .expect("Command failed to start");
}

fn jail(program: String, args: Vec<String>) -> c_int {
    let parent = id();
    let stack = stack_memory().as_mut_ptr() as *mut c_void;
    unsafe {
        clone(move |_| {
            run(program, args)
        }, stack, CLONE_NEWUTS | SIGCHLD, 0)
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

    let child_id = jail(args.next().unwrap(), args.collect());
    unsafe {
        wait(child_id);
    }
}
