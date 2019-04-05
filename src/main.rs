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
mod mount;

use args::Command;
use cgroup::{CgroupFactory, CgroupOptions};
use images::ImageRepository;
use jail::Jail;

const USAGE: &'static str = "Ruthless is a small application to run rootless, daemonless containers.

Possible commands:
ruthless run [image] [command] # Run the given command on the image.
ruthless image list # List images in the system
ruthless image delete [image] # Deletes image [image]";

fn run_command(image: &str, command: &Vec<String>, detach: bool) -> Result<(), Error> {
    let name = Uuid::new_v4().to_string();
    let image_repository = ImageRepository::new()?;
    let image_location = image_repository.get_image_location_for_process(image, name.as_str())?;
    let cgroup_factory = CgroupFactory::new(name, vec![CgroupOptions::PidsMax(10)]);
    let mut jail = Jail::new(detach);
    jail.run(command, image_location.to_str().unwrap(), cgroup_factory)?;
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
        Ok(Command::ListImages) => { list_images_command().unwrap() },
        Ok(Command::DeleteImage(image)) => { delete_image_command(image.as_str()).unwrap() }
        Ok(Command::Run { image, command, detach }) => {
            run_command(image.as_str(), &command, detach).unwrap();
        },
        Err(e) => {
            eprintln!("{}", e);
            eprintln!("{}", USAGE);
            exit(1)
        }
    }
}
