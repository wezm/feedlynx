use std::io::BufReader;
use std::path::PathBuf;
use std::{borrow::Cow, fs::File};

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

fn is_youtube(url: &URI) -> bool {
    let Some(host) = url.host() else {
        return false;
    };
    match host {
        uriparse::Host::IPv4Address(_) => false,
        uriparse::Host::IPv6Address(_) => false,
        uriparse::Host::RegisteredName(name) => matches!(
            name.as_str(),
            "www.youtube.com" | "youtu.be" | "m.youtube.com" | "youtube-nocookie.com"
        ),
    }
}

fn is_short(url: &URI) -> bool {
    let Some(host) = url.host() else {
        return false;
    };
    match host {
        uriparse::Host::IPv4Address(_) => false,
        uriparse::Host::IPv6Address(_) => false,
        uriparse::Host::RegisteredName(name) => name == "youtu.be",
    }
}

fn youtube_video_id<'a>(url: &'a URI) -> Option<Cow<'a, str>> {
    // Try for v param, fall back on 'v' segment
    let id = url
        .query()
        .and_then(|q| {
            form_urlencoded::parse(q.as_bytes()).find_map(|(key, value)| {
                if key == "v" {
                    Some(value)
                } else {
                    None
                }
            })
        })
        .or_else(|| match url.path().segments() {
            [first, id] if first == "v" => Some(Cow::Borrowed(id.as_str())),
            [id] if is_short(url) => Some(Cow::Borrowed(id.as_str())),
            _ => None,
        });

    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_id_direct() {
        let url = URI::try_from("https://www.youtube.com/watch?v=u1wfCnRINkE").unwrap();
        assert!(is_youtube(&url));
        assert_eq!(youtube_video_id(&url).unwrap(), "u1wfCnRINkE");
    }

    #[test]
    fn test_video_id_short() {
        let url = URI::try_from("https://youtu.be/u1wfCnRINkE").unwrap();
        assert!(is_youtube(&url));
        assert_eq!(youtube_video_id(&url).unwrap(), "u1wfCnRINkE");
    }

    #[test]
    fn test_video_id_fullscreen() {
        let url = URI::try_from("https://www.youtube.com/v/u1wfCnRINkE").unwrap();
        assert!(is_youtube(&url));
        assert_eq!(youtube_video_id(&url).unwrap(), "u1wfCnRINkE");
    }

    #[test]
    fn test_video_id_fullscreen_param() {
        let url = URI::try_from("https://www.youtube.com/v/u1wfCnRINkE?version=3").unwrap();
        assert!(is_youtube(&url));
        assert_eq!(youtube_video_id(&url).unwrap(), "u1wfCnRINkE");
    }

    #[test]
    fn test_video_id_channel_url() {
        let url =
            URI::try_from("https://www.youtube.com/channel/UCLi0H57HGGpAdCkVOb_ykVg").unwrap();
        assert!(is_youtube(&url));
        assert_eq!(youtube_video_id(&url), None);
    }
}
