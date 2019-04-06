use crate::cgroup::CgroupOptions;
use std::convert::TryFrom;
use std::str::FromStr;

const CPU_MAX_OPTION: &'static str = "--cpu-max=";
const CPU_WEIGHT_OPTION: &'static str = "--cpu-weight=";
const CPU_WEIGHT_NICE_OPTION: &'static str = "--cpu-weight-nice=";
const CPUSET_CPUS_OPTION: &'static str = "--cpuset-cpus=";
const CPUSET_CPUS_PARTITION_OPTION: &'static str = "--cpuset-cpus-partition=";
const CPUSET_MEMS_OPTION: &'static str = "--cpuset-mems=";
const IO_MAX_OPTION: &'static str = "--io-max=";
const IO_WEIGHT_OPTION: &'static str = "--io-weight=";
const MEMORY_HIGH_OPTION: &'static str = "--memory-high=";
const MEMORY_LOW_OPTION: &'static str = "--memory-low=";
const MEMORY_MAX_OPTION: &'static str = "--memory-max=";
const MEMORY_MIN_OPTION: &'static str = "--memory-min=";
const MEMORY_OOM_GROUP_OPTION: &'static str = "--memory-oom-group=";
const MEMORY_SWAP_MAX_OPTION: &'static str = "--memory-swap-max=";
const PIDS_MAX_OPTION: &'static str = "--pids-max=";
const RDMA_MAX_OPTION: &'static str = "--rdma-max=";

#[derive(Debug, Fail)]
pub(crate) enum ArgumentParsingError {
    #[fail(display = "You should pass at least one argument")]
    NotEnoughArguments,
    #[fail(display = "Unexpected command {}", 0)]
    UnexpectedCommand(String),
    #[fail(display = "Run command should contain an image")]
    MissingImage,
    #[fail(display = "An image subcommand is expected.")]
    NoImageSubCommand,
    #[fail(display = "Invalid image subcommand {}.", 0)]
    InvalidImageSubCommand(String),
    #[fail(display = "Missing image to delete.")]
    MissingImageToDelete,
    #[fail(display = "Can't parse argument {}.", 0)]
    CantParseNumber(String),
    #[fail(display = "Invalid argument {}.", 0)]
    InvalidArgument(String),
}

pub(crate) enum Command {
    DeleteImage(String),
    ListImages,
    Run {
        command: Vec<String>,
        detach: bool,
        image: String,
        resource_options: Vec<CgroupOptions>,
    },
}

fn parse_image_subcommand<I: Iterator<Item = String>>(
    mut source: I,
) -> Result<Command, ArgumentParsingError> {
    let subcommand = source
        .next()
        .ok_or(ArgumentParsingError::NoImageSubCommand)?;
    match subcommand.as_str() {
        "list" => Ok(Command::ListImages),
        "delete" => Ok(Command::DeleteImage(
            source
                .next()
                .ok_or(ArgumentParsingError::MissingImageToDelete)?,
        )),
        c => Err(ArgumentParsingError::InvalidImageSubCommand(c.to_owned())),
    }
}

macro_rules! handle_resource_option {
    ($number_type: ident, $actual_option: ident, $string_option: expr, $cgroup_option: ident, $resource_options: ident) => {
        let v = $number_type::from_str(
            &$actual_option.replace($string_option, "")
        )
            .map_err(|_| ArgumentParsingError::CantParseNumber($actual_option.to_owned()))?;
        $resource_options.push(CgroupOptions::$cgroup_option(v));
    }
}

macro_rules! handle_resource_option_string {
    ($actual_option: ident, $string_option: expr, $cgroup_option: ident, $resource_options: ident) => {
        $resource_options.push(CgroupOptions::$cgroup_option(
            $actual_option.replace($string_option, "").to_owned()
        ));
    }
}

macro_rules! handle_resource_option_string_number {
    ($number_type: ident, $actual_option: ident, $string_option: expr, $cgroup_option: ident, $resource_options: ident) => {
        let options: Vec<String> = $actual_option.replace($string_option, "").split(",").map(|v| v.to_owned()).collect();
        let period = $number_type::from_str(options[1].as_str())
            .map_err(|_| ArgumentParsingError::CantParseNumber($actual_option.to_owned()))?;
        $resource_options.push(CgroupOptions::$cgroup_option(options[0].clone(), period));
    }
}

fn parse_cgroup_option(argument: &str, resource_options: &mut Vec<CgroupOptions>) -> Result<(), ArgumentParsingError> {
    match argument {
        s if s.starts_with(CPU_MAX_OPTION) => {
            handle_resource_option_string_number!(usize, s, CPU_MAX_OPTION, CpuMax, resource_options);
        }
        s if s.starts_with(CPU_WEIGHT_OPTION) => {
            handle_resource_option!(usize, s, CPU_WEIGHT_OPTION, CpuWeight, resource_options);
        }
        s if s.starts_with(CPU_WEIGHT_NICE_OPTION) => {
            handle_resource_option!(isize, s, CPU_WEIGHT_NICE_OPTION, CpuWeightNice, resource_options);
        }
        s if s.starts_with(CPUSET_CPUS_OPTION) => {
            handle_resource_option_string!(s, CPUSET_CPUS_OPTION, CpusetCpus, resource_options);
        }
        s if s.starts_with(CPUSET_CPUS_PARTITION_OPTION) => {
            handle_resource_option_string!(s, CPUSET_CPUS_PARTITION_OPTION, CpusetCpusPartition, resource_options);
        }
        s if s.starts_with(CPUSET_MEMS_OPTION) => {
            handle_resource_option_string!(s, CPUSET_MEMS_OPTION, CpusetMems, resource_options);
        }
        s if s.starts_with(IO_MAX_OPTION) => {
            handle_resource_option_string!(s, IO_MAX_OPTION, IoMax, resource_options);
        }
        s if s.starts_with(IO_WEIGHT_OPTION) => {
            handle_resource_option_string_number!(usize, s, IO_WEIGHT_OPTION, IoWeight, resource_options);
        }
        s if s.starts_with(MEMORY_HIGH_OPTION) => {
            handle_resource_option_string!(s, MEMORY_HIGH_OPTION, MemoryHigh, resource_options);
        }
        s if s.starts_with(MEMORY_LOW_OPTION) => {
            handle_resource_option!(usize, s, MEMORY_LOW_OPTION, MemoryLow, resource_options);
        }
        s if s.starts_with(MEMORY_MAX_OPTION) => {
            handle_resource_option_string!(s, MEMORY_MAX_OPTION, MemoryMax, resource_options);
        }
        s if s.starts_with(MEMORY_MIN_OPTION) => {
            handle_resource_option!(usize, s, MEMORY_MIN_OPTION, MemoryMin, resource_options);
        }
        s if s.starts_with(MEMORY_OOM_GROUP_OPTION) => {
            handle_resource_option!(usize, s, MEMORY_OOM_GROUP_OPTION, MemoryOomGroup, resource_options);
        }
        s if s.starts_with(MEMORY_SWAP_MAX_OPTION) => {
            handle_resource_option_string!(s, MEMORY_SWAP_MAX_OPTION, MemorySwapMax, resource_options);
        }
        s if s.starts_with(PIDS_MAX_OPTION) => {
            handle_resource_option!(usize, s, PIDS_MAX_OPTION, PidsMax, resource_options);
        }
        s if s.starts_with(RDMA_MAX_OPTION) => {
            handle_resource_option_string!(s, RDMA_MAX_OPTION, RdmaMax, resource_options);
        }
        _ => Err(ArgumentParsingError::InvalidArgument(argument.to_owned()))?,
    };
    Ok(())
}

fn parse_run_subcommand<I: Iterator<Item = String>>(
    source: I,
) -> Result<Command, ArgumentParsingError> {
    let mut command = Vec::new();
    let mut detach = false;
    let mut image = None;
    let mut resource_options = Vec::new();
    for s in source {
        match (s.as_str(), &image) {
            ("-d", _) | ("--detach", _) if command.len() == 0 => {
                detach = true;
            }
            (s, _) if command.len() == 0 && s.starts_with("--") => {
                parse_cgroup_option(s, &mut resource_options)?;
            }
            (i, None) => image = Some(i.to_owned()),
            (c, Some(_)) => {
                command.push(c.to_owned());
            }
        }
    }
    Ok(Command::Run {
        command,
        detach,
        resource_options,
        image: image.ok_or(ArgumentParsingError::MissingImage)?,
    })
}

impl TryFrom<Vec<String>> for Command {
    type Error = ArgumentParsingError;

    fn try_from(args: Vec<String>) -> Result<Command, Self::Error> {
        let mut source = args.into_iter();
        let leading = source
            .next()
            .ok_or(ArgumentParsingError::NotEnoughArguments)?;
        match leading.as_str() {
            "image" => parse_image_subcommand(source),
            "run" => parse_run_subcommand(source),
            c => Err(ArgumentParsingError::UnexpectedCommand(c.to_owned())),
        }
    }
}
