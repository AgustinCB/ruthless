#[macro_use]
extern crate failure;
#[macro_use]
extern crate nix;

use failure::Error;
use std::convert::TryFrom;
use std::env::args;
use std::process::exit;

mod args;
mod cgroup;
mod images;
mod jail;
mod mount;

use args::Command;
use cgroup::Cgroup;
use images::ImageRepository;
use jail::Jail;

const USAGE: &'static str = "Ruthless is a small application to run rootless, daemonless containers.

Possible commands:
ruthless run [image] [command] # Run the given command on the image.
ruthless image list # List images in the system
ruthless image delete [image] # Deletes image [image]";

fn run_command(image: String, command: Vec<String>) -> Result<(), Error> {
    let image_repository = ImageRepository::new()?;
    let image_path = image_repository.get_image_location(&image)?.to_str().unwrap().to_owned();
    let cgroup = Cgroup::new()?;
    cgroup.set_max_processes(10)?;
    let mut jail = Jail::new(cgroup);
    jail.run(&command, image_path)?;
    Ok(())
}

fn list_images_command() -> Result<(), Error> {
    let image_repository = ImageRepository::new()?;
    for image in image_repository.get_images()? {
        println!("{}", image);
    }
    Ok(())
}

fn delete_image_command(image: String) -> Result<(), Error> {
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
        Ok(Command::DeleteImage(image)) => { delete_image_command(image).unwrap() }
        Ok(Command::Run { image, command }) => { run_command(image, command).unwrap() },
        Err(e) => {
            eprintln!("{}", e);
            eprintln!("{}", USAGE);
            exit(1)
        }
    }
}
