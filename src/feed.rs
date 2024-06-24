use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::{borrow::Cow, fs::File};

use atom_syndication::{self as atom, Generator};
use chrono::{DateTime, Utc};
use log::{info, trace};
use uriparse::URI;

use crate::webpage::WebPage;
use crate::{base62, Error};

const MAX_ENTRIES: usize = 50;

pub struct Feed {
    path: PathBuf,
    feed: atom_syndication::Feed,
}

impl Feed {
    pub fn read<P: Into<PathBuf>>(path: P) -> Result<Feed, Error> {
        let path = path.into();
        let file = File::open(&path)?;
        let feed = atom::Feed::read_from(BufReader::new(file))?;

        Ok(Feed { feed, path })
    }

    /// Construct a new, empty feed
    ///
    /// Elements like atom:id are populated new unique values.
    pub fn generate_new<P: Into<PathBuf>>(path: P) -> Self {
        let feed = atom::Feed {
            title: env!("CARGO_PKG_NAME").into(),
            updated: Utc::now().into(),
            ..Default::default()
        };
        let mut feed = Feed {
            feed,
            path: path.into(),
        };
        feed.set_feed_id();
        feed.set_feed_author();
        feed.set_generator();
        feed
    }

    pub fn add_url(&mut self, url: &URI, page: WebPage) {
        info!("Add {}", url);
        let now: DateTime<Utc> = Utc::now();

        // Add the new item
        let link = atom::Link {
            href: url.to_string(),
            rel: "alternate".to_string(),
            ..Default::default()
        };
        let authors = page
            .author
            .map(|author| {
                vec![atom::Person {
                    name: author,
                    ..Default::default()
                }]
            })
            .unwrap_or_default();
        let entry = atom::Entry {
            title: page.title.unwrap_or_else(|| "Untitled".to_string()).into(),
            id: unique_tag_id(),
            updated: now.into(),
            summary: Some(summary_for_url(url, page.description)),
            links: vec![link],
            authors,
            ..Default::default()
        };
        self.feed.entries.push(entry);
        self.set_generator();
        self.feed.set_updated(now);
    }

    /// Ensure there's no more than MAX_ENTRIES entries in the feed
    pub fn trim_entries(&mut self) {
        if self.feed.entries().len() <= MAX_ENTRIES {
            return;
        }

        let offset = self.feed.entries().len() - MAX_ENTRIES;
        let keep = self.feed.entries.split_off(offset);
        self.feed.entries = keep;
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

    /// Generate a new, unique id for this feed accoring to the [tag]
    /// URI scheme.
    ///
    /// [tag]: http://www.faqs.org/rfcs/rfc4151.html
    fn set_feed_id(&mut self) {
        self.feed.set_id(unique_tag_id());
    }

    /// Populate the author of the feed.
    ///
    /// Atom requires that the feed has an author or every entry does. Since we
    /// start off with an empty feed a default author is populated.
    fn set_feed_author(&mut self) {
        let author = atom::Person {
            name: env!("CARGO_PKG_NAME").to_string(),
            uri: Some(env!("CARGO_PKG_HOMEPAGE").to_string()),
            ..Default::default()
        };
        self.feed.set_authors(vec![author]);
    }

    /// Set the generator of the feed
    ///
    /// Uses the current package name and version.
    fn set_generator(&mut self) {
        let generator = Generator {
            value: env!("CARGO_PKG_NAME").to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            ..Default::default()
        };
        self.feed.set_generator(generator);
    }
}

fn summary_for_url(url: &URI, description: Option<String>) -> atom::Text {
    let video_id = is_youtube(url).then(|| youtube_video_id(url)).flatten();
    if let Some(video_id) = video_id {
        let mut summary = format!(
            r#"<iframe width="560" height="315" src="https://www.youtube.com/embed/{video_id}" title="YouTube video player" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share" referrerpolicy="strict-origin-when-cross-origin" allowfullscreen></iframe>"#,
        );
        if let Some(desc) = description.as_deref() {
            summary.push_str("<div>");
            summary.push_str(desc); // description is expected to be plain text
            summary.push_str("</div>");
        }
        atom::Text::html(summary)
    } else {
        match description {
            Some(desc) => atom::Text::plain(desc),
            None => atom::Text::html(format!(r#"<a href="{url}">{url}</a>"#)),
        }
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

fn unique_tag_id() -> String {
    // The specific id within the tag namespace
    let specific = base62::base62::<16>();
    format!("tag:feedlynx.7bit.org,2024:{specific}")
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
