#[macro_use]
extern crate failure;
#[macro_use]
extern crate nix;

use failure::Error;
use std::convert::TryFrom;
use std::env::args;
use std::process::exit;
use uuid::Uuid;

mod args;
mod cgroup;
mod images;
mod jail;
mod jaillogs;
mod mount;

use args::Command;
use cgroup::{CgroupFactory, CgroupOptions};
use images::ImageRepository;
use jail::Jail;

const USAGE: &'static str =
    "Ruthless is a small application to run rootless, daemonless containers.

Possible commands:
ruthless run [image] [command] # Run the given command on the image.
ruthless image list # List images in the system
ruthless image delete [image] # Deletes image [image]
ruthless help # See this message
ruthless help [command] # Describe what an specific command does";
const USAGE_RUN: &'static str = "Usage: ruthless run [options] [image] [command]

Run a container with the process [command] over the file system [image]

Options:

-d, --detach
\tDetach the process container and run it in the background.
-n [name], --name=[name]
\tRun the container with a specific name.
--cpu-max=[cpu max]
\tSet the value to the interface cpu.max.
--cpu-weight=[cpu weight]
\tSet the value to the interface cpu.weight.
--cpu-weight-nice=[cpu weight nice]
\tSet the value to the interface cpu.weight.nice.
--cpuset-cpus=[cpuset cpus]
\tSet the value to the interface cpuset.cpus.
--cpuset-cpus-partition=[cpuset cpus partition]
\tSet the value to the interface cpuset.cpus.partition.
--cpuset-mems=[cpuset mems]
\tSet the value to the interface cpu.mems.
--io-max=[io max]
\tSet the value to the interface io.max. This value requires spaces, so you should put the argument
in between quotes.
--io-weight=[io weight]
\tSet the value to the interface io.weight.
--memory-high=[memory high]
\tSet the value to the interface memory.high.
--memory-low=[memory low]
\tSet the value to the interface memory.low.
--memory-max=[memory max]
\tSet the value to the interface memory.max.
--memory-min=[memory min]
\tSet the value to the interface memory.min.
--memory-oom-group=[memory oom group]
\tSet the value to the interface memory.oom.group.
--memory-swap-max=[memory swap max]
\tSet the value to the interface memory.swap.max.
--pids-max=[pids max]
\tSet the value to the interface pids.max.
--rdma-max=[cpuset cpus partition]
\tSet the value to the interface rdma.max. This value requires spaces, so you should put the argument
in between quotes.";
const USAGE_IMAGE_LIST: &'static str = "Usage: ruthless image list

List all the images available right now in the repository.";
const USAGE_IMAGE_DELETE: &'static str = "Usage: ruthless image delete [image]

Attempts to delete the image [image] from the repository.";

fn run_command(
    image: &str,
    command: &Vec<String>,
    detach: bool,
    name: Option<String>,
    resource_options: &Vec<CgroupOptions>,
) -> Result<(), Error> {
    let name = name.unwrap_or(Uuid::new_v4().to_string());
    let image_repository = ImageRepository::new()?;
    let image_location = image_repository.get_image_location_for_process(image, name.as_str())?;
    let cgroup_factory = CgroupFactory::new(name, resource_options.clone());
    let mut jail = Jail::new(detach);
    jail.run(command, image_location.to_str().unwrap(), &cgroup_factory)?;
    Ok(())
}

fn list_images_command() -> Result<(), Error> {
    let image_repository = ImageRepository::new()?;
    for image in image_repository.get_images()? {
        println!("{}", image);
    }
    Ok(())
}

fn delete_image_command(image: &str) -> Result<(), Error> {
    let image_repository = ImageRepository::new()?;
    image_repository.delete_image(image)?;
    Ok(())
}

fn main() {
    let mut args = args();

    args.next();
    let arguments: Vec<String> = args.collect();
    match Command::try_from(arguments) {
        Ok(Command::Help(None)) => {
            println!("{}", USAGE);
        }
        Ok(Command::Help(Some(c))) => {
            match c.as_str() {
                "run" => println!("{}", USAGE_RUN),
                "image list" => println!("{}", USAGE_IMAGE_LIST),
                "image delete" => println!("{}", USAGE_IMAGE_DELETE),
                _ => panic!("Invalid command.\n\n{}", USAGE),
            }
        }
        Ok(Command::ListImages) => list_images_command().unwrap(),
        Ok(Command::DeleteImage(image)) => delete_image_command(image.as_str()).unwrap(),
        Ok(Command::Run {
            command,
            detach,
            image,
            name,
            resource_options,
        }) => {
            run_command(image.as_str(), &command, detach, name, &resource_options).unwrap();
        }
        Err(e) => {
            eprintln!("{}", e);
            eprintln!("{}", USAGE);
            exit(1)
        }
    }
}
