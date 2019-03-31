use std::convert::TryFrom;

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
}

pub(crate) enum Command {
    DeleteImage(String),
    ListImages,
    Run {
        command: Vec<String>,
        image: String,
    },
}

fn parse_image_subcommand<I: Iterator<Item=String>>(
    mut source: I
) -> Result<Command, ArgumentParsingError> {
    let subcommand = source.next().ok_or(ArgumentParsingError::NoImageSubCommand)?;
    match subcommand.as_str() {
        "list" => Ok(Command::ListImages),
        "delete" => {
            Ok(Command::DeleteImage(source.next().ok_or(ArgumentParsingError::MissingImageToDelete)?))
        }
        c => Err(ArgumentParsingError::InvalidImageSubCommand(c.to_owned()))
    }
}

impl TryFrom<Vec<String>> for Command {
    type Error = ArgumentParsingError;

    fn try_from(args: Vec<String>) -> Result<Command, Self::Error> {
        let mut source = args.into_iter();
        let leading = source.next().ok_or(ArgumentParsingError::NotEnoughArguments)?;
        match leading.as_str() {
            "image" => { parse_image_subcommand(source) },
            "run" => {
                let image = source.next().ok_or(ArgumentParsingError::MissingImage)?.clone();
                let command = source.map(|s| s.clone()).collect();
                Ok(Command::Run { image, command })
            }
            c => Err(ArgumentParsingError::UnexpectedCommand(c.to_owned()))
        }
    }
}