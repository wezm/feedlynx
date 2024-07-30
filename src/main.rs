mod cli;

use std::{
    env::{self, VarError},
    ffi::OsString,
    process::ExitCode,
    sync::Arc,
    thread,
};

use env_logger::Env;
use feedlynx::{
    base62::base62, webpage, Feed, FeedToken, PrivateToken, Server, DEFAULT_ADDR, DEFAULT_PORT,
};
use log::{error, info, trace};

use crate::cli::Command;

const ENV_ADDRESS: &str = "FEEDLYNX_ADDRESS";
const ENV_PORT: &str = "FEEDLYNX_PORT";
const ENV_PRIVATE_TOKEN: &str = "FEEDLYNX_PRIVATE_TOKEN";
const ENV_FEED_TOKEN: &str = "FEEDLYNX_FEED_TOKEN";
const ENV_LOG: &str = "FEEDLYNX_LOG";

struct Config {
    addr: String,
    port: u16,
    private_token: PrivateToken,
    feed_token: FeedToken,
}

fn main() -> ExitCode {
    match env::var_os(ENV_LOG) {
        None => env::set_var(ENV_LOG, "info"),
        Some(_) => {}
    }
    env_logger::init_from_env(Env::new().filter(ENV_LOG));

    let cmd = match cli::parse_args() {
        Ok(cmd) => cmd,
        Err(err) => {
            eprintln!("Unable to parse arguments: {err}");
            return ExitCode::FAILURE;
        }
    };

    let feed_path = match cmd {
        Command::Serve(feed_path) => feed_path,
        Command::GenToken => {
            generate_token();
            return ExitCode::SUCCESS;
        }
        Command::Fetch(url) => {
            fetch_webpage(url);
            return ExitCode::SUCCESS;
        }
        Command::Exit(code) => {
            return code;
        }
    };

    // Create the feed file if it does not exist
    if !feed_path.exists() {
        info!("Creating initial feed at {}", feed_path.display());
        let feed = Feed::generate_new(&feed_path);
        match feed.save() {
            Ok(()) => {}
            Err(err) => {
                eprintln!("Unable to save initial feed: {err}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        // Ensure existing feed can be read before starting the server
        match Feed::read(&feed_path) {
            Ok(_feed) => {}
            Err(err) => {
                eprintln!("Unable to read feed at {}: {err}", feed_path.display());
                return ExitCode::FAILURE;
            }
        }
    }

    let config = match read_config() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Unable to read configuration: {err}");
            eprintln!(
                "{} and {} must both be set to a 32 character string",
                ENV_PRIVATE_TOKEN, ENV_FEED_TOKEN
            );
            eprintln!("Generate tokens with: {} gen-token", env!("CARGO_BIN_NAME"));
            return ExitCode::FAILURE;
        }
    };

    // This sets the signal mask, which has to happen before the server starts its threads
    // so that they inherit the mask
    let signals = match feedlynx::SignalHandle::new() {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("Unable to set signal mask: {err}");
            return ExitCode::FAILURE;
        }
    };

    let server = match Server::new(
        (config.addr.clone(), config.port),
        config.private_token,
        config.feed_token,
        feed_path,
    ) {
        Ok(server) => Arc::new(server),
        Err(err) => {
            eprintln!(
                "Unable to start http server on {}:{}: {}",
                config.addr, config.port, err
            );
            return ExitCode::FAILURE;
        }
    };

    // Spawn thread to wait for signals
    let server2 = Arc::clone(&server);
    let join_handle = thread::Builder::new()
        .name("signal-handler".to_string())
        .spawn(move || {
            trace!("waiting for signals...");
            match signals.block_until_signalled() {
                Ok(()) => trace!("signalled!"),
                Err(err) => error!("Waiting for signals failed: {err}"),
            }
            server2.shutdown();
        })
        .unwrap(); // NOTE(unwrap): if thread fails to spawn panic seems reasonable

    info!(
        "HTTP server running on: http://{}:{}",
        config.addr, config.port
    );
    server.handle_requests();
    trace!("server finished handling requests");

    // NOTE(unwrap): will propagate panic from thread (if applicable)
    join_handle.join().unwrap();

    ExitCode::SUCCESS
}

fn read_config() -> Result<Config, String> {
    let server_addr = env::var(ENV_ADDRESS).unwrap_or_else(|_| String::from(DEFAULT_ADDR));
    let server_port = env::var(ENV_PORT)
        .ok()
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

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

fn fetch_webpage(url: Option<OsString>) {
    let Some(url) = url.as_ref().and_then(|os| os.to_str()) else {
        error!("missing url");
        return;
    };

    match webpage::fetch(url) {
        Ok(page) => {
            println!(
                "title: {:?}\ndescription: {:?}",
                page.title, page.description
            )
        }
        Err(err) => {
            println!("unable to fetch page: {err}")
        }
    }
}
