use std::{
    io::Cursor,
    path::{Path, PathBuf},
    process::Child,
    time::Duration,
};

use atom_syndication as atom;
use form_urlencoded as form;

use feedlynx::base62::base62;
use minreq::Request;

const PRIVATE_TOKEN: &str = "TestTestTestTestTestTestTest1234";
const FEED_TOKEN: &str = "FeedFeedFeedFeedFeedFeedFeedFeed";
const PORT: &str = "8003"; // Use a different port so it doesn't hit the dev server accidentely
const ADDRESS: &str = "127.0.0.1:8003";

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
    let feed_path = std::env::temp_dir().join(format!("feed.{rand}.xml",));
    assert!(!feed_path.exists());
    let feed_path = RmOnDrop::new(feed_path);

    let mut binary = test_bin::get_test_bin("feedlynx");
    binary
        .envs([
            ("FEEDLYNX_PRIVATE_TOKEN", PRIVATE_TOKEN),
            ("FEEDLYNX_FEED_TOKEN", FEED_TOKEN),
            ("FEEDLYNX_PORT", PORT),
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

    // Ensure the server is up and accepting requests
    let mut attempt = 0;
    loop {
        match minreq::get(format!("http://{}/", ADDRESS)).send() {
            Ok(res) => {
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
    let (feed, _) = fetch_feed();
    assert_eq!(feed.entries().len(), 0);

    // Add a link to the feed and check again
    let url = "http://example.com/";
    add_link(url);
    let (feed, last_modified) = fetch_feed();
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

    // Check 304
    assert_eq!(fetch_feed_conditional(&last_modified), 304);

    // Check missing content type in POST is rejected
    let res = prepare_add_link(url, PRIVATE_TOKEN)
        .send()
        .expect("POST /add without content type failed");
    assert_eq!(res.status_code, 400);
    assert!(res.as_str().unwrap().contains("Missing Content-Type"));

    // Check wrong content type in POST is rejected
    let res = prepare_add_link(url, PRIVATE_TOKEN)
        .with_header("Content-Type", "application/json")
        .send()
        .expect("POST /add with wrong content type failed");
    assert_eq!(res.status_code, 415);
    assert!(res.as_str().unwrap().contains("Unsupported media type"));

    // Check that token is required to add link
    add_link_wrong_token(url);

    // Check that token is required to fetch feed
    let res = minreq::get(format!("http://{}/feed/{}", ADDRESS, "invalid-token"))
        .send()
        .expect("GET /feed with invalid token failed");
    assert_eq!(res.status_code, 404);
}

fn fetch_feed() -> (atom::Feed, String) {
    let res = minreq::get(format!("http://{}/feed/{}", ADDRESS, FEED_TOKEN))
        .send()
        .expect("GET /feed failed");
    assert_eq!(res.status_code, 200);

    // Get the Last-Modified header
    let last_modified = res
        .headers
        .get("last-modified")
        .expect("Last-Modifed header is set");

    let xml = res.as_str().unwrap();
    let feed = atom::Feed::read_from(Cursor::new(xml)).expect("failed to parse feed");
    (feed, last_modified.to_owned())
}

fn fetch_feed_conditional(last_modified: &str) -> i32 {
    dbg!(last_modified);
    let res = minreq::get(format!("http://{}/feed/{}", ADDRESS, FEED_TOKEN))
        .with_header("If-Modified-Since", last_modified)
        .send()
        .expect("GET /feed failed");
    res.status_code
}

fn prepare_add_link(url: &str, token: &str) -> Request {
    let body = form::Serializer::new(String::new())
        .append_pair("url", url)
        .append_pair("token", token)
        .finish();
    minreq::post(format!("http://{}/add", ADDRESS)).with_body(body)
}

fn add_link(url: &str) {
    let res = prepare_add_link(url, PRIVATE_TOKEN)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .expect("POST /add failed");
    assert_eq!(res.status_code, 201);
}

fn add_link_wrong_token(url: &str) {
    let res = prepare_add_link(url, "nope-token")
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .expect("POST /add with wrong token failed");
    assert_eq!(res.status_code, 401);
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
