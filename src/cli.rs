use std::convert::Infallible;
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::ExitCode;

use feedlynx::{DEFAULT_ADDR, DEFAULT_PORT};
use pico_args::Arguments;

pub enum Command {
    Serve(PathBuf),
    GenToken,
    Fetch(Option<OsString>),
    Exit(ExitCode),
}

pub fn parse_args() -> Result<Command, pico_args::Error> {
    let mut pargs = Arguments::from_env();
    if pargs.contains(["-V", "--version"]) {
        return print_version();
    } else if pargs.contains(["-h", "--help"]) {
        return print_help();
    }

    let arg0 = pargs.opt_free_from_os_str(osstring)?;
    match arg0 {
        Some(arg) if arg == "gen-token" => Ok(Command::GenToken),
        Some(arg) if arg == "fetch" => Ok(Command::Fetch(pargs.opt_free_from_os_str(osstring)?)),
        Some(arg) => Ok(Command::Serve(PathBuf::from(arg))),
        None => {
            eprintln!("Usage: {} path/to/feed.xml", env!("CARGO_BIN_NAME"));
            Ok(Command::Exit(ExitCode::FAILURE))
        }
    }
}

fn osstring(s: &OsStr) -> Result<OsString, Infallible> {
    Ok(s.to_os_string())
}

fn print_version() -> Result<Command, pico_args::Error> {
    println!("{}", version_string());
    Ok(Command::Exit(ExitCode::SUCCESS))
}

pub fn print_help() -> Result<Command, pico_args::Error> {
    println!(
        "{}

{bin} collects links to read or watch later in an RSS feed.

USAGE:
    {bin} [OPTIONS] FEED_PATH

OPTIONS:
    -h, --help
            Prints this help information

    -V, --version
            Prints version information

ENVIRONMENT:

    Required:

        FEEDLYNX_PRIVATE_TOKEN
            Used to authenticate requests to add a new link.

        FEEDLYNX_FEED_TOKEN
            Used in the path to the generated feed.

    Optional:

        FEEDLYNX_ADDRESS
            The address to serve on, default `{addr}`.

        FEEDLYNX_PORT
            The port to serve on, default `{port}`.

        FEEDLYNX_LOG
            Controls the log level and filtering.

AUTHOR
    {}

SEE ALSO
    https://github.com/wezm/feedlynx  Source code and issue tracker.",
        version_string(),
        env!("CARGO_PKG_AUTHORS"),
        bin = env!("CARGO_PKG_NAME"),
        addr = DEFAULT_ADDR,
        port = DEFAULT_PORT
    );
    Ok(Command::Exit(ExitCode::SUCCESS))
}

fn version_string() -> String {
    format!(
        "{} version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )
}
