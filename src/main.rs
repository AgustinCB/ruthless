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
mod btrfs_send;
mod cgroup;
mod images;
mod jail;
mod jaillogs;
mod mount;
mod oci_image;

use crate::cgroup::{get_active_cgroups, terminate_cgroup_processes};
use crate::oci_image::{export, OCIImage};
use args::Command;
use cgroup::{CgroupFactory, CgroupOptions};
use images::ImageRepository;
use jail::Jail;
use std::fs::read_to_string;

const USAGE: &str = "Ruthless is a small application to run rootless, daemonless containers.

Possible commands:
ruthless run [image] [command] # Run the given command on the image.
ruthless logs [container] # Show logs of a container
ruthless container delete [container] # Kill running containers
ruthless container list # List all running containers
ruthless image list # List images in the system
ruthless image delete [image] # Deletes image [image]
ruthless export [image] [tarball] # Export [image] into the location [tarball]
ruthless import [tarball] # Import a OCI compatible tarball into the image repository
ruthless help # See this message
ruthless help [command] # Describe what an specific command does";
const USAGE_CONTAINER_DELETE: &str = "Usage: ruthless container delete [container]

Attempts to delete a running container by killing the processes running in it.";
const USAGE_CONTAINER_LIST: &str = "Usage: ruthless container list

List all the containers that are currently running in the system";
const USAGE_RUN: &str = "Usage: ruthless run [options] [image] [command]

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
const USAGE_IMAGE_LIST: &str = "Usage: ruthless image list

List all the images available right now in the repository.";
const USAGE_IMAGE_DELETE: &str = "Usage: ruthless image delete [image]

Attempts to delete the image [image] from the repository.";
const USAGE_IMPORT: &str = "Usage: ruthless import [tarball]

Import a OCI tarball image into the ruthless image repository.";
const USAGE_EXPORT: &str = "Usage: ruthless export [image] [tarball]

Export an image into a docker compatible tarball.";
const USAGE_LOGS: &str = "Usage: ruthless logs [container]

Prints the standard output of the container into the current standard output and the standard error
of the container into the current standard error.";

fn run_command(
    image: &str,
    command: &[String],
    detach: bool,
    name: Option<String>,
    resource_options: &[CgroupOptions],
) -> Result<(), Error> {
    let name = name.unwrap_or_else(|| Uuid::new_v4().to_string());
    let image_repository = ImageRepository::new()?;
    let image_location = image_repository.get_image_location_for_process(image, name.as_str())?;
    let cgroup_factory = CgroupFactory::new(name, resource_options.to_owned());
    let mut jail = Jail::new(detach);
    jail.run(command, image_location.to_str().unwrap(), &cgroup_factory)?;
    Ok(())
}

fn delete_container_command(container: &str) -> Result<(), Error> {
    terminate_cgroup_processes(container)
}

fn delete_image_command(image: &str) -> Result<(), Error> {
    let image_repository = ImageRepository::new()?;
    image_repository.delete_image(image)?;
    Ok(())
}

fn export_command(image: &str, tarball: &str) -> Result<(), Error> {
    let image_repository = ImageRepository::new()?;
    export(&image_repository, image, tarball)?;
    Ok(())
}

fn import_command(tarball: &str) -> Result<(), Error> {
    let image_repository = ImageRepository::new()?;
    let oci_image = OCIImage::new(tarball)?;
    oci_image.import(&image_repository)
}

fn list_containers_command() -> Result<(), Error> {
    get_active_cgroups()?.iter().for_each(|c| println!("{}", c));
    Ok(())
}

fn list_images_command() -> Result<(), Error> {
    let image_repository = ImageRepository::new()?;
    for image in image_repository.get_images()? {
        println!("{}", image);
    }
    Ok(())
}

fn show_container_logs(container: &str) -> Result<(), Error> {
    let image_repository = ImageRepository::new()?;
    let logs_path = image_repository.get_logs_path(container);
    println!("{}", read_to_string(logs_path.join("stdout"))?.trim());
    eprintln!("{}", read_to_string(logs_path.join("stderr"))?.trim());
    Ok(())
}

fn main() {
    let mut args = args();

    args.next();
    let arguments: Vec<String> = args.collect();
    match Command::try_from(arguments) {
        Ok(Command::DeleteContainer(container)) => {
            delete_container_command(container.as_str()).unwrap()
        }
        Ok(Command::DeleteImage(image)) => delete_image_command(image.as_str()).unwrap(),
        Ok(Command::Export(image, tarball)) => {
            export_command(image.as_str(), tarball.as_str()).unwrap()
        }
        Ok(Command::Help(None)) => {
            println!("{}", USAGE);
        }
        Ok(Command::Help(Some(c))) => match c.as_str() {
            "container list" => println!("{}", USAGE_CONTAINER_LIST),
            "container delete" => println!("{}", USAGE_CONTAINER_DELETE),
            "image delete" => println!("{}", USAGE_IMAGE_DELETE),
            "image list" => println!("{}", USAGE_IMAGE_LIST),
            "export" => println!("{}", USAGE_EXPORT),
            "import" => println!("{}", USAGE_IMPORT),
            "logs" => println!("{}", USAGE_LOGS),
            "run" => println!("{}", USAGE_RUN),
            _ => panic!("Invalid command.\n\n{}", USAGE),
        },
        Ok(Command::Import(tarball)) => import_command(tarball.as_str()).unwrap(),
        Ok(Command::ListContainers) => list_containers_command().unwrap(),
        Ok(Command::ListImages) => list_images_command().unwrap(),
        Ok(Command::Logs(c)) => show_container_logs(&c).unwrap(),
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
