use crate::cgroup::Cgroup;
use failure::Error;
use libc::{c_int, close, read, pipe, write, __errno_location};
use std::ffi::c_void;
use std::sync::Arc;

#[derive(Debug, Fail)]
enum RunArgsError {
    #[fail(display="Error creating pipes: {}", errno)]
    PipeCreationError { errno: c_int },
    #[fail(display="Error writing to pipe: {}", errno)]
    PipeWritingError { errno: c_int },
    #[fail(display="Error reading from pipe: {}", errno)]
    PipeReadingError { errno: c_int },
    #[fail(display="Error closing pipe: {}", errno)]
    PipeClosingError { errno: c_int },
}

pub(crate) struct RunArgs {
    pub(crate) args: Vec<String>,
    pub(crate) cgroup: Arc<Cgroup>,
    pub(crate) image: String,
    pipes: [c_int; 2],
}

impl RunArgs {
    pub(crate) fn new(args: Vec<String>, image: String) -> Result<RunArgs, Error> {
        let mut pipes = [0;2];
        let res = unsafe { pipe(pipes.as_mut_ptr()) };
        let cgroup = Arc::new(Cgroup::new()?);
        if res != 0 {
            Err(Error::from(RunArgsError::PipeCreationError { errno: unsafe { *__errno_location() } }))
        } else {
            Ok(RunArgs {
                args,
                cgroup,
                image,
                pipes,
            })
        }
    }

    pub(crate) fn signal_child(&self) -> Result<(), Error> {
        let res = unsafe {
            write(self.pipes[1], vec![0].as_ptr() as *const c_void, 1)
        };
        if res > 0 {
            Ok(())
        } else {
            Err(Error::from(RunArgsError::PipeWritingError { errno: unsafe { *__errno_location() } }))
        }
    }

    pub(crate) fn wait_for_parent(&self) -> Result<(), Error> {
        let mut content = [0;1];
        let res = unsafe {
            read(self.pipes[0], content.as_mut_ptr() as *mut c_void, 1)
        };
        if res > 0 {
            Ok(())
        } else {
            Err(Error::from(RunArgsError::PipeReadingError { errno: unsafe { *__errno_location() } }))
        }
    }

    fn close_pipe(&self, id: usize) -> Result<(), Error> {
        let res = unsafe {
            close(self.pipes[id])
        };
        if res == 0 {
            Ok(())
        } else {
            Err(Error::from(RunArgsError::PipeClosingError { errno: unsafe { *__errno_location() } }))
        }
    }
}

impl Drop for RunArgs {
    fn drop(&mut self) {
        self.close_pipe(0).unwrap();
        self.close_pipe(1).unwrap();
    }
}
