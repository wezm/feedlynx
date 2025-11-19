use std::io::{BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::{borrow::Cow, fs::File};
use std::{fs, mem};

use atom_syndication::{self as atom, Entry, Generator};
use chrono::{DateTime, TimeDelta, Utc};
use log::{info, trace};
use uriparse::URI;

use crate::webpage::WebPage;
use crate::{base62, Error};

pub const MIN_ENTRIES: usize = 50;
pub const TRIM_AGE: TimeDelta = TimeDelta::days(30);

pub struct Feed {
    path: PathBuf,
    feed: atom_syndication::Feed,
}

pub enum AddResult {
    Added,
    Duplicate,
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

    pub fn add_url_if_new(&mut self, url: &URI, page: WebPage) -> AddResult {
        let url_str = url.to_string();
        let duplicate = self
            .feed
            .entries()
            .iter()
            .any(|entry| entry.links().iter().any(|link| link.href() == &url_str));

        if duplicate {
            AddResult::Duplicate
        } else {
            self.add_url(url, page);
            AddResult::Added
        }
    }

    fn add_url(&mut self, url: &URI, page: WebPage) {
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

    /// Trim entries older than `trim_age`, but keep `min_entries`.
    pub fn trim_entries(&mut self) {
        trim_entries(&mut self.feed.entries, MIN_ENTRIES, TRIM_AGE);
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

    /// Generate a new, unique id for this feed according to the [tag]
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

fn trim_entries(entries: &mut Vec<Entry>, min_entries: usize, trim_age: TimeDelta) {
    if entries.len() <= min_entries {
        return;
    }

    // Sort by age (oldest first) so that old items are dropped first.
    // This is not really necessary since the entries should be in this order already,
    // but we'll be sure.
    entries.sort_by(|a, b| a.updated().cmp(b.updated()));

    let now: DateTime<Utc> = Utc::now();
    let mut num_trim = entries.len() - min_entries;
    let new_entries = mem::take(entries);
    *entries = new_entries
        .into_iter()
        .filter(|entry| {
            if num_trim == 0 {
                return true;
            }

            let age = now - <DateTime<Utc>>::from(*entry.updated());
            if age > trim_age {
                info!("Trim entry {}: {}", entry.id(), entry.title().as_str());
                num_trim -= 1;
                false
            } else {
                true
            }
        })
        .collect();
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
    use chrono::FixedOffset;

    use super::*;

    fn test_entry(title: atom::Text, updated: DateTime<FixedOffset>) -> Entry {
        atom::Entry {
            title,
            id: unique_tag_id(),
            updated,
            summary: Some("Summary".into()),
            authors: vec![atom::Person {
                name: "Author".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

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

    // entry is old enough to be trimmed, but is retained because there's less than
    // min entries present.
    #[test]
    fn test_trim_less_than_min() {
        let now = Utc::now();
        let updated = (now - TimeDelta::seconds(5)).into();
        let entry = test_entry("Test".into(), updated);
        let mut entries = vec![entry];
        trim_entries(&mut entries, 3, TimeDelta::seconds(1));
        assert_eq!(entries.len(), 1);
    }

    // There's more than min entries items present, but they're all younger
    // than trim age.
    #[test]
    fn text_trim_young() {
        let now = Utc::now();
        let updated = (now - TimeDelta::seconds(5)).into();
        let entry = test_entry("Test".into(), updated);
        let mut entries = vec![entry; 3];
        trim_entries(&mut entries, 2, TimeDelta::seconds(10));
        assert_eq!(entries.len(), 3);
    }

    // There's more than min entries items present but only one is old enough
    // to trim.
    #[test]
    fn test_trim_one_old() {
        let now = Utc::now();
        let entry = test_entry("Test".into(), (now - TimeDelta::seconds(5)).into());
        let mut entries = vec![entry; 3];
        let entry = test_entry("Old".into(), (now - TimeDelta::seconds(15)).into());
        entries.push(entry);
        trim_entries(&mut entries, 2, TimeDelta::seconds(10));
        assert_eq!(entries.len(), 3);
    }

    // There's more than min entries items present and all are old enough
    // to trim. Only enough to drop to min entries should be dropped. Oldest
    // items should be dropped.
    #[test]
    fn test_trim_all_old() {
        let now = Utc::now();
        // - Test 1: 11 secs
        // - Test 2: 12 secs
        // - Test 3: 13 secs
        // - Test 4: 14 secs
        // Normally entries would not be ordered like this since new items are appended to the end,
        // which means they'll be ordered oldest to newest.
        let mut entries = (0..4)
            .map(|i| {
                test_entry(
                    format!("Test {}", i + 1).into(),
                    (now - TimeDelta::seconds(11 + i)).into(),
                )
            })
            .collect::<Vec<_>>();
        trim_entries(&mut entries, 2, TimeDelta::seconds(10));
        let titles = entries
            .iter()
            .map(|entry| entry.title().as_str())
            .collect::<Vec<_>>();

        // 1 and 2 should be retained as they are the youngest.
        assert_eq!(titles, ["Test 2", "Test 1"]);
    }
}
