use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::{borrow::Cow, fs::File};

use atom_syndication::{self as atom, Generator};
use chrono::{DateTime, Utc};
use log::{debug, info, trace};
use uriparse::URI;

use crate::{embed, Error};

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

    pub fn empty<P: Into<PathBuf>>(path: P) -> Self {
        let content = embed!("default.xml");
        let feed = atom::Feed::read_from(BufReader::new(content.as_bytes()))
            .expect("default feed is invalid");
        let mut feed = Feed {
            feed,
            path: path.into(),
        };
        feed.set_generator();
        feed
    }

    pub fn add_url(&mut self, url: &URI) {
        info!("Add {}", url);
        let now: DateTime<Utc> = Utc::now();

        // Add the new item
        let link = atom::Link {
            href: url.to_string(),
            rel: "alternate".to_string(),
            ..Default::default()
        };
        let entry = atom::Entry {
            title: "Title".into(), // FIXME: pull from POST body
            id: format!("{}", url),
            updated: now.into(),
            summary: Some(summary_for_url(url)),
            links: vec![link],
            // authors: vec![author],
            ..Default::default()
        };
        self.feed.entries.push(entry);

        // Write
        self.set_generator();

        self.feed.set_updated(now);

        // write to the feed to a writer
        //feed.write_to(sink()).unwrap();

        // convert the feed to a string
        let string = self.feed.to_string();
        println!("{}", string);
    }

    pub fn save(&self) -> Result<(), Error> {
        let tmp_path = self.path.with_extension("tmp");

        // Wrap in block so that tmp_file is dropped before calling rename
        {
            // Write out the file entirely
            let tmp_file = File::create(&tmp_path)?;
            let writer = BufWriter::new(tmp_file);
            let mut writer = self.feed.write_to(writer)?;
            writer.flush()?;
            trace!("Wrote {}", tmp_path.display())
        }

        // Move into place atomically
        trace!("Move {} -> {}", tmp_path.display(), self.path.display());
        fs::rename(tmp_path, &self.path).map_err(Error::from)
    }

    fn set_generator(&mut self) {
        let generator = Generator {
            value: env!("CARGO_PKG_NAME").to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            ..Default::default()
        };
        self.feed.set_generator(generator);
    }
}

fn summary_for_url(url: &URI) -> atom::Text {
    let video_id = is_youtube(url).then(|| youtube_video_id(url)).flatten();
    if let Some(video_id) = video_id {
        atom::Text::html(format!(
            r#"<iframe width="560" height="315" src="https://www.youtube.com/embed/{video_id}" title="YouTube video player" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share" referrerpolicy="strict-origin-when-cross-origin" allowfullscreen></iframe>"#,
        ))
    } else {
        atom::Text::html(r#"<a href="{url}">{url}</a>"#)
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
