pub mod base62;
mod feed;
pub(crate) mod minrandom;
mod server;

use std::{fmt, io};

pub use server::Server;

#[derive(Debug)]
pub enum Error {
    Feed(atom_syndication::Error),
    Io(io::Error),
}

pub struct PrivateToken(pub String);

pub struct FeedToken(pub String);

impl PartialEq<str> for PrivateToken {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Feed(err) => write!(f, "feed error: {err}"),
            Error::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl From<atom_syndication::Error> for Error {
    fn from(err: atom_syndication::Error) -> Self {
        Error::Feed(err)
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl std::error::Error for Error {}
