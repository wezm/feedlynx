use std::{
    env::{self, VarError},
    path::PathBuf,
    process::{self, ExitCode},
};

use vidlater::{base62::base62, Feed, FeedToken, PrivateToken, Server};

const ENV_ADDRESS: &str = "VIDLATER_ADDRESS";
const ENV_PORT: &str = "VIDLATER_PORT";
const ENV_PRIVATE_TOKEN: &str = "VIDLATER_PRIVATE_TOKEN";
const ENV_FEED_TOKEN: &str = "VIDLATER_FEED_TOKEN";

struct Config {
    addr: String,
    port: u16,
    private_token: PrivateToken,
    feed_token: FeedToken,
}

fn main() -> ExitCode {
    let arg = env::args_os().skip(1).next();

    let feed_path = match arg {
        Some(arg) if arg == "gen-token" => {
            generate_token();
            return ExitCode::SUCCESS;
        }
        Some(arg) => PathBuf::from(arg),
        None => {
            eprintln!("Usage: {} path/to/feed.xml", env!("CARGO_BIN_NAME"));
            return ExitCode::FAILURE;
        }
    };

    // Create the feed file if it does not exist
    if !feed_path.exists() {
        let feed = Feed::empty(&feed_path);
        feed.save().expect("FIXME");
    }

    let config = read_config().expect("FIXME: config");
    let server = match Server::new(
        (config.addr.clone(), config.port),
        config.private_token,
        config.feed_token,
        feed_path,
    ) {
        Ok(server) => server,
        Err(err) => {
            eprintln!(
                "ERROR: Unable to start http server on {}:{}: {}",
                config.addr, config.port, err
            );
            process::exit(1);
        }
    };

    println!(
        "INFO: http server running on http://{}:{}",
        config.addr, config.port
    );
    server.handle_requests();

    ExitCode::SUCCESS
}

fn read_config() -> Result<Config, String> {
    let server_addr = env::var(ENV_ADDRESS).unwrap_or_else(|_| String::from("0.0.0.0"));
    let server_port = env::var(ENV_PORT)
        .ok()
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(8001);

    let private_token = read_token(ENV_PRIVATE_TOKEN).map(PrivateToken)?;
    let feed_token = read_token(ENV_FEED_TOKEN).map(FeedToken)?;

    Ok(Config {
        addr: server_addr,
        port: server_port,
        private_token,
        feed_token,
    })
}

fn read_token(name: &str) -> Result<String, String> {
    let token = env::var(name).map_err(|err| match err {
        VarError::NotPresent => format!("{} environment variable is not set", name),
        VarError::NotUnicode(_) => format!("{} environment variable is not valid utf-8", name),
    })?;

    if token.len() < 32 {
        return Err(format!("{} is too short", name));
    }

    Ok(token)
}

/// Generate and print a base62 encoded token
fn generate_token() {
    println!("{}", base62::<32>());
}
