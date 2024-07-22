use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use httpdate::fmt_http_date;
use log::{debug, error, info, log_enabled, warn};
use tiny_http::{Header, HeaderField, Method, Request, Response, StatusCode};
use tinyjson::JsonValue;
use uriparse::URI;

use crate::feed::Feed;
use crate::webpage::WebPage;
use crate::{embed, webpage, FeedToken, PrivateToken};

// HTTP status codes
const CREATED: u16 = 201;
const NOT_MODIFIED: u16 = 304;
const BAD_REQUEST: u16 = 400;
const UNAUTHORIZED: u16 = 401;
const NOT_FOUND: u16 = 404;
const PAYLOAD_TOO_LARGE: u16 = 413;
const UNSUPPORTED_MEDIA_TYPE: u16 = 415;
const INTERNAL_SERVER_ERROR: u16 = 500;

/// The maximum size in bytes that the server will accept in a POST to /add
const MAX_POST_BODY: usize = 1_048_576; // 1MiB

// Pre-parsed headers for reading
static CONTENT_TYPE: OnceLock<HeaderField> = OnceLock::new();
static HOST: OnceLock<HeaderField> = OnceLock::new();
static IF_MODIFIED_SINCE: OnceLock<HeaderField> = OnceLock::new();
static LAST_MODIFIED: OnceLock<HeaderField> = OnceLock::new();
static USER_AGENT: OnceLock<HeaderField> = OnceLock::new();

// Pre-parsed headers for writing
static ACCESS_CONTROL_ORIGIN_STAR: OnceLock<Header> = OnceLock::new();
static ATOM_CONTENT_TYPE: OnceLock<Header> = OnceLock::new();
static HTML_CONTENT_TYPE: OnceLock<Header> = OnceLock::new();
static JSON_CONTENT_TYPE: OnceLock<Header> = OnceLock::new();

pub struct Server {
    server: tiny_http::Server,
    private_token: PrivateToken,
    feed_path: RwLock<PathBuf>,
    feed_route: String,
}

struct StatusError(StatusCode, &'static str);

impl Server {
    pub fn new<A>(
        addr: A,
        private_token: PrivateToken,
        feed_token: FeedToken,
        feed_path: PathBuf,
    ) -> Result<Server, Box<dyn Error + Send + Sync + 'static>>
    where
        A: ToSocketAddrs,
    {
        tiny_http::Server::http(addr).map(|server| Server {
            server,
            private_token,
            feed_path: RwLock::new(feed_path),
            feed_route: format!("/feed/{}", feed_token.0),
        })
    }

    pub fn handle_requests(&self) {
        // initialize statics
        let _ = CONTENT_TYPE.set("Content-Type".parse().unwrap());
        let _ = HOST.set("Host".parse().unwrap());
        let _ = IF_MODIFIED_SINCE.set("If-Modified-Since".parse().unwrap());
        let _ = LAST_MODIFIED.set("Last-Modified".parse().unwrap());
        let _ = USER_AGENT.set("User-Agent".parse().unwrap());

        let _ = ACCESS_CONTROL_ORIGIN_STAR.set("Access-Control-Allow-Origin: *".parse().unwrap());
        let _ = ATOM_CONTENT_TYPE.set("Content-type: application/atom+xml".parse().unwrap());
        let _ = HTML_CONTENT_TYPE.set("Content-type: text/html; charset=utf-8".parse().unwrap());
        let _ = JSON_CONTENT_TYPE.set("Content-type: application/json".parse().unwrap());

        info!(
            "Feed available at: http://{}{}",
            self.server.server_addr(),
            self.feed_route
        );

        for mut request in self.server.incoming_requests() {
            let response = match (request.method(), request.url()) {
                (Method::Get, "/") => {
                    let body = self.index(&request);
                    Response::from_string(body)
                        .with_header(HTML_CONTENT_TYPE.get().cloned().unwrap())
                }
                // TODO: Handle query args (I.e. ignore them?)
                // This branch has a different response type so we have to call respond and continue
                // instead of falling through to the code at the bottom.
                (Method::Get, path) if path == self.feed_route => {
                    let feed_path = self.feed_path.read().expect("poisoned");
                    match File::open(&*feed_path) {
                        Ok(file) => {
                            let modified = file.metadata().and_then(|meta| meta.modified()).ok();
                            let if_modified_since = request
                                .headers()
                                .iter()
                                .find(|&header| &header.field == IF_MODIFIED_SINCE.get().unwrap())
                                .and_then(|header| {
                                    httpdate::parse_http_date(header.value.as_str()).ok()
                                });

                            match (modified, if_modified_since) {
                                // Send 304 response
                                (Some(modified), Some(ifs)) if not_modified(modified, ifs) => {
                                    // https://www.rfc-editor.org/rfc/rfc7232#page-18 suggests Last-Modified should
                                    // still be included in the 304 response
                                    let response =
                                        Response::empty(NOT_MODIFIED).with_header(Header {
                                            field: LAST_MODIFIED.get().cloned().unwrap(),
                                            // NOTE(unwrap): we always expect ASCII from fmt_http_date
                                            value: fmt_http_date(modified).parse().unwrap(),
                                        });
                                    self.log_request(&request, response.status_code());
                                    match request.respond(response) {
                                        Ok(()) => {}
                                        Err(err) => error!("Failed to send response: {err}"),
                                    }
                                    continue;
                                }
                                _ => {}
                            }

                            // Send 200 response with File
                            let mut response = Response::from_file(file)
                                .with_header(ATOM_CONTENT_TYPE.get().cloned().unwrap());
                            if let Some(modified) = modified {
                                response = response.with_header(Header {
                                    field: LAST_MODIFIED.get().cloned().unwrap(),
                                    // NOTE(unwrap): we always expect ASCII from fmt_http_date
                                    value: fmt_http_date(modified).parse().unwrap(),
                                });
                            }
                            self.log_request(&request, response.status_code());
                            match request.respond(response) {
                                Ok(()) => {}
                                Err(err) => error!("Failed to send response: {err}"),
                            }
                            continue;
                        }
                        Err(err) => {
                            error!("Unable to open feed file: {}", err);
                            Response::from_string(embed!("500.html"))
                                .with_status_code(INTERNAL_SERVER_ERROR)
                        }
                    }
                }
                (Method::Post, "/add") => match self.add(&mut request) {
                    Ok(()) => Response::from_string("Added\n")
                        .with_header(ACCESS_CONTROL_ORIGIN_STAR.get().cloned().unwrap())
                        .with_status_code(CREATED),
                    Err(StatusError(status, error)) => {
                        Response::from_string(format!("Failed: {error}\n"))
                            .with_header(ACCESS_CONTROL_ORIGIN_STAR.get().cloned().unwrap())
                            .with_status_code(status)
                    }
                },
                (Method::Post, "/info") => match self.info(&mut request) {
                    Ok(info) => {
                        let json = JsonValue::Object(info);
                        // NOTE(unwrap): io::Error should not happen when writing to a String
                        Response::from_string(tinyjson::stringify(&json).unwrap())
                            .with_header(JSON_CONTENT_TYPE.get().cloned().unwrap())
                            .with_header(ACCESS_CONTROL_ORIGIN_STAR.get().cloned().unwrap())
                    }
                    Err(StatusError(status, error)) => {
                        let map = IntoIterator::into_iter([
                            ("status".to_string(), JsonValue::from("error".to_string())),
                            ("message".to_string(), JsonValue::from(error.to_string())),
                        ])
                        .collect();
                        let json = JsonValue::Object(map);
                        // NOTE(unwrap): io::Error should not happen when writing to a String
                        Response::from_string(tinyjson::stringify(&json).unwrap())
                            .with_header(JSON_CONTENT_TYPE.get().cloned().unwrap())
                            .with_header(ACCESS_CONTROL_ORIGIN_STAR.get().cloned().unwrap())
                            .with_status_code(status)
                    }
                },
                _ => Response::from_string(embed!("404.html"))
                    .with_header(HTML_CONTENT_TYPE.get().cloned().unwrap())
                    .with_status_code(NOT_FOUND),
            };

            self.log_request(&request, response.status_code());

            match request.respond(response) {
                Ok(()) => {}
                Err(err) => error!("Failed to send response: {err}"),
            }
        }
    }

    fn index(&self, request: &Request) -> String {
        let logo = embed!("../feedlynx.svg");
        let host = request
            .headers()
            .iter()
            .find_map(|header| {
                (&header.field == HOST.get().unwrap()).then(|| Cow::from(header.value.as_str()))
            })
            .unwrap_or_else(|| Cow::from(self.server.server_addr().to_string()));
        let feed_url = format!("http://{host}/feed/FEEDLYNX_FEED_TOKEN");
        embed!("index.html")
            .into_owned()
            .replace("{{logo}}", &logo)
            .replace("{{feed}}", &feed_url)
    }

    fn add(&self, request: &mut Request) -> Result<(), StatusError> {
        self.validate_request(request)?;
        let body = read_body(request)?;

        // Parse the form submission and extract the token and url
        let mut token = None;
        let mut url = None;
        let mut title = None;

        form_urlencoded::parse(&body).for_each(|(key, value)| match &*key {
            "token" => token = Some(value),
            "url" => url = Some(value),
            "title" => title = Some(value),
            _ => {}
        });

        let token = token.ok_or_else(|| StatusError::new(BAD_REQUEST, "Missing token"))?;

        // Validate token
        if self.private_token != *token {
            return Err(StatusError::new(UNAUTHORIZED, "Invalid token"));
        }

        // Parse URL
        let Some(url) = url.as_ref().and_then(|u| URI::try_from(u.as_ref()).ok()) else {
            return Err(StatusError::new(BAD_REQUEST, "Invalid URL"));
        };

        // Fetch the page for extra metadata
        let mut page = match webpage::fetch(url.to_string()) {
            Ok(page) => page,
            Err(err) => {
                warn!("Failed to fetch {}: {err}", url);
                WebPage::default()
            }
        };

        // Use the title supplied in the request if its longer than that fetched from the page.
        // This aims to handle cases like YouTube where fetching the video URL returns a
        // Challenge page to prove you aren't a bot with a generic title and description.
        if let Some(title) = &title {
            webpage::set_if_longer(&mut page.title, title);
        }

        // Add to the feed
        let feed_path = self.feed_path.write().expect("poisoned");
        let mut feed = Feed::read(&*feed_path).map_err(|err| {
            error!("Unable to read feed file: {err}");
            StatusError::new(INTERNAL_SERVER_ERROR, "Unable to read feed file")
        })?;
        feed.add_url(&url, page);
        feed.trim_entries();
        feed.save().map_err(|err| {
            error!("Unable to save feed: {err}");
            StatusError::new(INTERNAL_SERVER_ERROR, "Error saving feed file")
        })
    }

    fn info(&self, request: &mut Request) -> Result<HashMap<String, JsonValue>, StatusError> {
        self.validate_request(request)?;
        let body = read_body(request)?;

        // Parse the form submission and extract the token
        let mut token = None;

        form_urlencoded::parse(&body).for_each(|(key, value)| match &*key {
            "token" => token = Some(value),
            _ => {}
        });

        let token = token.ok_or_else(|| StatusError::new(BAD_REQUEST, "Missing token"))?;

        // Validate token
        if self.private_token != *token {
            return Err(StatusError::new(UNAUTHORIZED, "Invalid token"));
        }

        Ok(IntoIterator::into_iter([
            ("status".to_string(), JsonValue::from("ok".to_string())),
            (
                "version".to_string(),
                JsonValue::from(env!("CARGO_PKG_VERSION").to_string()),
            ),
        ])
        .collect())
    }

    fn validate_request(&self, request: &Request) -> Result<(), StatusError> {
        // Extract required headers
        let content_type = request
            .headers()
            .iter()
            .find(|&header| &header.field == CONTENT_TYPE.get().unwrap())
            .ok_or_else(|| StatusError::new(BAD_REQUEST, "Missing Content-Type"))?;

        if content_type.value != "application/x-www-form-urlencoded" {
            return Err(StatusError::new(
                UNSUPPORTED_MEDIA_TYPE,
                "Unsupported media type",
            ));
        }

        Ok(())
    }

    fn log_request(&self, request: &Request, status: StatusCode) {
        if log_enabled!(log::Level::Debug) {
            let host = request
                .remote_addr()
                .map(|sock| Cow::from(sock.to_string()))
                .unwrap_or_else(|| Cow::from("-"));
            let user_agent = request.headers().iter().find_map(|header| {
                (&header.field == USER_AGENT.get().unwrap()).then(|| header.value.as_str())
            });
            debug!(
                "{} \"{} {}\" {} \"{}\"",
                host,
                request.method().as_str(),
                request.url(),
                status.0,
                user_agent.unwrap_or("-")
            )
        }
    }

    pub fn shutdown(&self) {
        self.server.unblock();
    }
}

fn read_body(request: &mut Request) -> Result<Vec<u8>, StatusError> {
    let mut buf = [0; 8 * 1024];
    let mut body = Vec::new();
    let reader = request.as_reader();
    loop {
        match reader.read(&mut buf) {
            // EOF reached; body successfully read
            Ok(0) => break,
            Ok(n) => {
                body.extend_from_slice(&buf[..n]);
            }
            // Retry
            Err(ref err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => {
                error!("Unable to read POST body: {err}");
                return Err(StatusError::new(
                    INTERNAL_SERVER_ERROR,
                    "Unable to read POST body",
                ));
            }
        }
        if body.len() > MAX_POST_BODY {
            let msg = "POST body exceeded maximum size";
            error!("{msg}");
            return Err(StatusError::new(PAYLOAD_TOO_LARGE, msg));
        }
    }

    Ok(body)
}

/// Compare mtime and If-Modified-Since value to determine if content has changed.
///
/// The values are compared as seconds since the UNIX epoch because SystemTime
/// carries seconds and nanoseconds and the nano seconds can cause a direct
/// comparison to fail. Also it's an opaque type so we can't set the nanoseconds
/// to zero either.
fn not_modified(modified: SystemTime, if_modified_since: SystemTime) -> bool {
    let Ok(modified) = modified.duration_since(UNIX_EPOCH) else {
        return false;
    };
    let Ok(if_modified) = if_modified_since.duration_since(UNIX_EPOCH) else {
        return false;
    };
    modified.as_secs() <= if_modified.as_secs()
}

impl StatusError {
    fn new<C: Into<StatusCode>>(code: C, message: &'static str) -> Self {
        StatusError(code.into(), message)
    }
}
