use std::{
    collections::HashMap,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Child,
    time::Duration,
};

use atom_syndication as atom;
use form_urlencoded as form;

use feedlynx::base62::base62;
use minreq::Request;
use tinyjson::{JsonParser, JsonValue};

const PRIVATE_TOKEN: &str = "TestTestTestTestTestTestTest1234";
const FEED_TOKEN: &str = "FeedFeedFeedFeedFeedFeedFeedFeed";
const PORT: u16 = 8003; // Use a different port so it doesn't hit the dev server accidentely

struct RmOnDrop(PathBuf);

struct StopOnDrop(Child);

impl RmOnDrop {
    fn new(path: PathBuf) -> Self {
        RmOnDrop(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for RmOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        if let Err(err) = self.0.kill() {
            panic!("failed to stop server: {err}");
        }
    }
}

#[test]
fn server() {
    let rand = base62::<8>();
    let feed_path = std::env::temp_dir().join(format!("feed.{rand}.xml"));
    assert!(!feed_path.exists());
    let feed_path = RmOnDrop::new(feed_path);

    let mut binary = test_bin::get_test_bin("feedlynx");
    binary
        .envs([
            ("FEEDLYNX_PRIVATE_TOKEN", PRIVATE_TOKEN),
            ("FEEDLYNX_FEED_TOKEN", FEED_TOKEN),
            ("FEEDLYNX_PORT", &PORT.to_string()),
            ("FEEDLYNX_LOG", "debug"),
        ])
        .arg(feed_path.path());
    let mut child = binary
        .spawn()
        .map(StopOnDrop)
        .expect("failed to spawn server");
    std::thread::sleep(Duration::from_millis(250));
    let status = child.0.try_wait().expect("unable to get status");
    if let Some(code) = status {
        panic!("server failed to start ({})", code)
    }

    let address = format!("127.0.0.1:{}", PORT);

    // Ensure the server is up and accepting requests
    let mut attempt = 0;
    loop {
        match minreq::get(format!("http://{}/", address)).send() {
            Ok(res) => {
                assert_eq!(res.status_code, 200);

                let content_type = res
                    .headers
                    .get("content-type")
                    .expect("Content-Type header is set");
                assert_eq!(content_type, "text/html; charset=utf-8");

                let body = res.as_str().unwrap();
                assert!(body.contains("Feed available at"));
                break;
            }
            Err(err) => {
                attempt += 1;
                if attempt > 2 {
                    panic!("GET / failed: {err}");
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }

    // Fetch the feed
    let (feed, _) = fetch_feed(&address);
    assert_eq!(feed.entries().len(), 0);

    // Fetch info from the server
    let info = get_info(None, &address);
    assert!(info.is_object());
    let obj: &HashMap<_, _> = info.get().unwrap();
    assert_eq!(obj["status"].get::<String>().unwrap(), "ok");
    assert_eq!(
        obj["version"].get::<String>().unwrap(),
        env!("CARGO_PKG_VERSION")
    );

    // Fetch info from the server with charset
    let info = get_info(Some("utf-8"), &address);
    assert!(info.is_object());
    let obj: &HashMap<_, _> = info.get().unwrap();
    assert_eq!(obj["status"].get::<String>().unwrap(), "ok");
    assert_eq!(
        obj["version"].get::<String>().unwrap(),
        env!("CARGO_PKG_VERSION")
    );

    // Fetch info from the server without token
    let info = get_info_no_token(&address);
    assert!(info.is_object());
    let obj: &HashMap<_, _> = info.get().unwrap();
    assert_eq!(obj["status"].get::<String>().unwrap(), "error");
    assert_eq!(obj["message"].get::<String>().unwrap(), "Missing token");

    // Fetch info from the server with wrong token
    let info = get_info_wrong_token(&address);
    assert!(info.is_object());
    let obj: &HashMap<_, _> = info.get().unwrap();
    assert_eq!(obj["status"].get::<String>().unwrap(), "error");
    assert_eq!(obj["message"].get::<String>().unwrap(), "Invalid token");

    // Add a link to the feed and check again
    let url = "http://example.com/";
    add_link(url, &address);
    let (feed, _last_modified) = fetch_feed(&address);
    assert_eq!(feed.entries().len(), 1);
    assert_eq!(
        feed.entries()
            .last()
            .unwrap()
            .links()
            .first()
            .unwrap()
            .href(),
        url
    );

    // Add a duplicate link to the feed and check it is not added
    let url = "http://example.com/";
    let body = add_link(url, &address);
    let (feed, last_modified) = fetch_feed(&address);
    assert_eq!(feed.entries().len(), 1);
    assert!(body.contains("Duplicate"));

    // Check 304
    assert_eq!(fetch_feed_conditional(&last_modified, &address), 304);

    // Check 304
    assert_eq!(fetch_feed_conditional(&last_modified, &address), 304);

    // Check missing content type in POST is rejected
    let res = prepare_add_link(url, PRIVATE_TOKEN, &address)
        .send()
        .expect("POST /add without content type failed");
    assert_eq!(res.status_code, 400);
    assert!(res.as_str().unwrap().contains("Missing Content-Type"));

    // Check wrong content type in POST is rejected
    let res = prepare_add_link(url, PRIVATE_TOKEN, &address)
        .with_header("Content-Type", "application/json")
        .send()
        .expect("POST /add with wrong content type failed");
    assert_eq!(res.status_code, 415);
    assert!(res.as_str().unwrap().contains("Unsupported media type"));

    // Check unsupported charset in POST is rejected
    let res = prepare_add_link(url, PRIVATE_TOKEN, &address)
        .with_header(
            "Content-Type",
            "application/x-www-form-urlencoded; charset=UTF-16",
        )
        .send()
        .expect("POST /add with wrong charset failed");
    assert_eq!(res.status_code, 415);
    assert!(res.as_str().unwrap().contains("Unsupported character set"));

    // Check that token is required to add link
    add_link_wrong_token(url, &address);

    // Check that token is required to fetch feed
    let res = minreq::get(format!("http://{}/feed/{}", address, "invalid-token"))
        .send()
        .expect("GET /feed with invalid token failed");
    assert_eq!(res.status_code, 404);
}

#[test]
fn trim() {
    let rand = base62::<8>();
    let feed_path = std::env::temp_dir().join(format!("feed.{rand}.xml"));
    assert!(!feed_path.exists());
    let sample_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("sample.xml");
    fs::copy(sample_path, &feed_path).expect("unable to copy sample feed");
    let feed_path = RmOnDrop::new(feed_path);

    let mut binary = test_bin::get_test_bin("feedlynx");
    binary
        .envs([
            ("FEEDLYNX_PRIVATE_TOKEN", PRIVATE_TOKEN),
            ("FEEDLYNX_FEED_TOKEN", FEED_TOKEN),
            ("FEEDLYNX_PORT", &(PORT + 1).to_string()),
            ("FEEDLYNX_LOG", "debug"),
        ])
        .arg(feed_path.path());
    let mut child = binary
        .spawn()
        .map(StopOnDrop)
        .expect("failed to spawn server");
    std::thread::sleep(Duration::from_millis(250));
    let status = child.0.try_wait().expect("unable to get status");
    if let Some(code) = status {
        panic!("server failed to start ({})", code)
    }

    let address = format!("127.0.0.1:{}", PORT + 1);

    // Ensure the server is up and accepting requests
    let mut attempt = 0;
    loop {
        match minreq::get(format!("http://{}/", address)).send() {
            Ok(res) => {
                assert_eq!(res.status_code, 200);

                let content_type = res
                    .headers
                    .get("content-type")
                    .expect("Content-Type header is set");
                assert_eq!(content_type, "text/html; charset=utf-8");

                let body = res.as_str().unwrap();
                assert!(body.contains("Feed available at"));
                break;
            }
            Err(err) => {
                attempt += 1;
                if attempt > 2 {
                    panic!("GET / failed: {err}");
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }

    let ids = [
        "tag:feedlynx.7bit.org,2024:BBPslb1dYm9x1KOz2",
        "tag:feedlynx.7bit.org,2024:BBPslb1dYm9x1KOz11",
        "tag:feedlynx.7bit.org,2024:BBPslb1dYm9x1KOz12",
        "tag:feedlynx.7bit.org,2024:BBPslb1dYm9x1KOz13",
    ];

    // Before adding a new link check that that items we expect to be removed are present.
    let (feed, _last_modified) = fetch_feed(&address);
    assert_eq!(feed.entries().len(), 53);
    ids.iter().for_each(|&id| {
        feed.entries()
            .iter()
            .find(|entry| entry.id() == id)
            .expect(&format!("expected to find entry with id: {}", id));
    });

    // Add a link to the feed, which should trigger trimming, check that the trim worked.
    let url = "http://example.com/";
    add_link(url, &address);
    let (feed, _last_modified) = fetch_feed(&address);
    assert_eq!(feed.entries().len(), 50);

    // Check that these entries were removed
    let removed = ids.iter().all(|&id| {
        feed.entries()
            .iter()
            .find(|entry| entry.id() == id)
            .is_none()
    });
    assert!(removed);
}

fn fetch_feed(address: &str) -> (atom::Feed, String) {
    let res = minreq::get(format!("http://{}/feed/{}", address, FEED_TOKEN))
        .send()
        .expect("GET /feed failed");
    assert_eq!(res.status_code, 200);

    // Get the Content-Type
    let content_type = res
        .headers
        .get("content-type")
        .expect("Content-Type header is set");
    assert_eq!(content_type, "application/atom+xml");

    // Get the Last-Modified header
    let last_modified = res
        .headers
        .get("last-modified")
        .expect("Last-Modified header is set");

    let xml = res.as_str().unwrap();
    let feed = atom::Feed::read_from(Cursor::new(xml)).expect("failed to parse feed");
    (feed, last_modified.to_owned())
}

fn fetch_feed_conditional(last_modified: &str, address: &str) -> i32 {
    dbg!(last_modified);
    let res = minreq::get(format!("http://{}/feed/{}", address, FEED_TOKEN))
        .with_header("If-Modified-Since", last_modified)
        .send()
        .expect("GET /feed failed");
    res.status_code
}

fn prepare_add_link(url: &str, token: &str, address: &str) -> Request {
    let body = form::Serializer::new(String::new())
        .append_pair("url", url)
        .append_pair("token", token)
        .finish();
    minreq::post(format!("http://{}/add", address)).with_body(body)
}

fn add_link(url: &str, address: &str) -> String {
    let res = prepare_add_link(url, PRIVATE_TOKEN, address)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .expect("POST /add failed");
    assert_eq!(res.status_code, 201);
    res.as_str()
        .expect("unable to get body as string")
        .to_string()
}

fn add_link_wrong_token(url: &str, address: &str) {
    let res = prepare_add_link(url, "nope-token", address)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .expect("POST /add with wrong token failed");
    assert_eq!(res.status_code, 401);
}

fn prepare_get_info(token: &str, address: &str) -> Request {
    let body = form::Serializer::new(String::new())
        .append_pair("token", token)
        .finish();
    minreq::post(format!("http://{}/info", address)).with_body(body)
}

fn get_info(charset: Option<&str>, address: &str) -> JsonValue {
    let mut content_type = "application/x-www-form-urlencoded".to_string();
    if let Some(charset) = charset {
        content_type.push_str("; charset=");
        content_type.push_str(charset);
    }

    let res = prepare_get_info(PRIVATE_TOKEN, address)
        .with_header("Content-Type", content_type)
        .send()
        .expect("POST /info failed");

    assert_eq!(res.status_code, 200);

    // Get the Content-Type
    let content_type = res
        .headers
        .get("content-type")
        .expect("Content-Type header is set");
    assert_eq!(content_type, "application/json");

    JsonParser::new(res.as_str().unwrap().chars())
        .parse()
        .expect("unable to parse info")
}

fn get_info_wrong_token(address: &str) -> JsonValue {
    let res = prepare_get_info("nope-token", address)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .expect("POST /info with wrong token failed");
    assert_eq!(res.status_code, 401);

    // Get the Content-Type
    let content_type = res
        .headers
        .get("content-type")
        .expect("Content-Type header is set");
    assert_eq!(content_type, "application/json");

    JsonParser::new(res.as_str().unwrap().chars())
        .parse()
        .expect("unable to parse info")
}

fn get_info_no_token(address: &str) -> JsonValue {
    let res = minreq::post(format!("http://{}/info", address))
        .with_body("")
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .expect("POST /info with no token failed");
    assert_eq!(res.status_code, 400);

    // Get the Content-Type
    let content_type = res
        .headers
        .get("content-type")
        .expect("Content-Type header is set");
    assert_eq!(content_type, "application/json");

    JsonParser::new(res.as_str().unwrap().chars())
        .parse()
        .expect("unable to parse info")
}

// https://github.com/MichaelMcDonnell/test_bin/blob/3f6ded86bbc46171e8d092fc592c65fa94abc60b/src/lib.rs
mod test_bin {
    //! A module for getting the crate binary in an integration test.
    //!
    //! If you are writing a command-line interface app then it is useful to write
    //! an integration test that uses the binary. You most likely want to launch the
    //! binary and inspect the output. This module lets you get the binary so it can
    //! be tested.
    //!
    //! # Examples
    //!
    //! basic usage:
    //!
    //! ```no_run
    //! let output = test_bin::get_test_bin("my_cli_app")
    //!     .output()
    //!     .expect("Failed to start my_binary");
    //! assert_eq!(
    //!     String::from_utf8_lossy(&output.stdout),
    //!     "Output from my CLI app!\n"
    //! );
    //! ```
    //!
    //! Refer to the [`std::process::Command` documentation](https://doc.rust-lang.org/std/process/struct.Command.html)
    //! for how to pass arguments, check exit status and more.

    /// Returns the crate's binary as a `Command` that can be used for integration
    /// tests.
    ///
    /// # Arguments
    ///
    /// * `bin_name` - The name of the binary you want to test.
    ///
    /// # Remarks
    ///
    /// It panics on error. This is by design so the test that uses it fails.
    pub fn get_test_bin(bin_name: &str) -> std::process::Command {
        // Create full path to binary
        let mut path = get_test_bin_dir();
        path.push(bin_name);
        path.set_extension(std::env::consts::EXE_EXTENSION);

        assert!(path.exists());

        // Create command
        std::process::Command::new(path.into_os_string())
    }

    /// Returns the directory of the crate's binary.
    ///
    /// # Remarks
    ///
    /// It panics on error. This is by design so the test that uses it fails.
    fn get_test_bin_dir() -> std::path::PathBuf {
        // Cargo puts the integration test binary in target/debug/deps
        let current_exe =
            std::env::current_exe().expect("Failed to get the path of the integration test binary");
        let current_dir = current_exe
            .parent()
            .expect("Failed to get the directory of the integration test binary");

        let test_bin_dir = current_dir
            .parent()
            .expect("Failed to get the binary folder");
        test_bin_dir.to_owned()
    }
}
