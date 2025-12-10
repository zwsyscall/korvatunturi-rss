pub static VERSION: &str = env!("CARGO_PKG_VERSION");

pub enum ServerCommand {
    AddFeed(String),
    RemoveFeed(String),
    Ping,
    Version,
}

#[derive(Debug)]
pub enum CommandParseError {
    MissingKeyword,
    UnknownKeyword,
    NotLongEnough,
    MissingLink,
}

impl TryFrom<String> for ServerCommand {
    type Error = CommandParseError;

    fn try_from(cmd: String) -> Result<Self, Self::Error> {
        let mut cmd_iter = cmd.split(" ").into_iter();
        if let Some(w) = cmd_iter.next() {
            let command = match w {
                "feed" => match cmd_iter.next() {
                    Some("add") => ServerCommand::AddFeed(
                        cmd_iter
                            .next()
                            .ok_or(CommandParseError::MissingLink)?
                            .to_string(),
                    ),
                    Some("remove") => ServerCommand::RemoveFeed(
                        cmd_iter
                            .next()
                            .ok_or(CommandParseError::MissingLink)?
                            .to_string(),
                    ),
                    Some(_) => return Err(CommandParseError::UnknownKeyword),
                    None => return Err(CommandParseError::NotLongEnough),
                },
                "ping" => ServerCommand::Ping,
                "version" => ServerCommand::Version,
                _ => return Err(CommandParseError::MissingKeyword),
            };
            return Ok(command);
        }
        return Err(CommandParseError::NotLongEnough);
    }
}

impl ServerCommand {
    pub fn to_string(self) -> String {
        match self {
            ServerCommand::AddFeed(feed) => {
                format!("feed add {}", feed)
            }
            ServerCommand::RemoveFeed(feed) => {
                format!("feed remove {}", feed)
            }
            ServerCommand::Ping => "ping".to_string(),
            ServerCommand::Version => "version".to_string(),
        }
    }

    pub fn format_reply(&self) -> Option<String> {
        match &self {
            ServerCommand::AddFeed(_) => None,
            ServerCommand::RemoveFeed(_) => None,
            ServerCommand::Ping => Some("Pong".to_string()),
            ServerCommand::Version => Some(VERSION.to_string()),
        }
    }
}
