use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use atom_syndication::{self as atom, Generator};
use chrono::{DateTime, Utc};
use uriparse::URI;

use crate::Error;

pub struct Feed {
    path: PathBuf,
    feed: atom_syndication::Feed,
}

impl Feed {
    pub fn new<P: Into<PathBuf>>(path: P) -> Result<Feed, Error> {
        let path = path.into();

        // Read
        let file = File::open(&path)?;
        let feed = atom::Feed::read_from(BufReader::new(file))?;

        Ok(Feed { feed, path })
    }

    pub fn add_url(&mut self, url: &URI) {
        // Write
        let generator = Generator {
            value: env!("CARGO_PKG_NAME").to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            ..Default::default()
        };
        self.feed.set_generator(generator);

        //let sys_time = SystemTime::now();
        //let timestamp=sys_time.duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let local: DateTime<Utc> = Utc::now();

        //let now = FixedDateTime::from_timestamp(timestamp, 0).unwrap();
        self.feed.set_updated(local);

        // write to the feed to a writer
        //feed.write_to(sink()).unwrap();

        // convert the feed to a string
        let string = self.feed.to_string();
        println!("{}", string);
    }

    pub fn save(&self) -> Result<(), Error> {
        // let cookie_store_path = cookie_store_path()?;
        // let cookie_store_tmp_path = cookie_store_path.with_extension("tmp");

        // // Ensure the directory the cookie file is stored in exists
        // let config_dir = cookie_store_path.parent().ok_or_else(|| {
        //     Error::Io(io::Error::new(
        //         io::ErrorKind::Other,
        //         "unable to find parent dir of cookie file",
        //     ))
        // })?;

        // if !config_dir.exists() {
        //     DirBuilder::new().recursive(true).create(config_dir)?;
        // }

        // {
        //     // Write out the file entirely
        //     let mut tmp_file = File::create(&cookie_store_tmp_path)?;
        //     self.http.save_cookies(&mut tmp_file)?;
        // }

        // // Move into place atomically
        // fs::rename(cookie_store_tmp_path, cookie_store_path).map_err(Error::from)
        todo!()
    }
}
