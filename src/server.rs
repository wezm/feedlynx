use std::borrow::Cow;
use std::error::Error;
use std::fs::File;
use std::io;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::sync::RwLock;

use httpdate::fmt_http_date;
use log::{debug, error, info, log_enabled, warn};
use tiny_http::{Header, HeaderField, Method, Request, Response, StatusCode};
use uriparse::URI;

use crate::feed::Feed;
use crate::webpage::WebPage;
use crate::{embed, webpage, FeedToken, PrivateToken};

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

pub struct Server {
    server: tiny_http::Server,
    private_token: PrivateToken,
    feed_path: PathBuf,
    feed_route: String,
    feed_lock: RwLock<()>,
    content_type_field: HeaderField,
    user_agent_field: HeaderField,
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
            feed_path,
            feed_route: format!("/feed/{}", feed_token.0),
            feed_lock: RwLock::new(()),
            content_type_field: "Content-Type".parse().unwrap(),
            user_agent_field: "User-Agent".parse().unwrap(),
        })
    }

    pub fn handle_requests(&self) {
        let html_content_type: Header = "Content-type: text/html; charset=utf-8".parse().unwrap();
        let atom_content_type: Header = "Content-type: application/atom+xml".parse().unwrap();
        let last_modified_field: HeaderField = "Last-Modified".parse().unwrap();
        let if_modified_since_field: HeaderField = "If-Modified-Since".parse().unwrap();

        info!("feed available at {}", self.feed_route);

        for mut request in self.server.incoming_requests() {
            let response = match (request.method(), request.url()) {
                (Method::Get, "/") => Response::from_string(embed!("index.html"))
                    .with_header(html_content_type.clone()),
                // TODO: Handle query args (I.e. ignore them?)
                // This branch has a different response type so we have to call respond and continue
                // instead of falling through to the code at the bottom.
                (Method::Get, path) if path == self.feed_route => {
                    let _lock = self.feed_lock.read().expect("poisioned");
                    match File::open(&self.feed_path) {
                        Ok(file) => {
                            let modified = file.metadata().and_then(|meta| meta.modified()).ok();
                            let if_modified_since = request
                                .headers()
                                .iter()
                                .find(|&header| header.field == if_modified_since_field)
                                .and_then(|header| {
                                    httpdate::parse_http_date(header.value.as_str()).ok()
                                });

                            match (modified, if_modified_since) {
                                // Send 304 response
                                (Some(modified), Some(ifs)) if modified <= ifs => {
                                    // https://www.rfc-editor.org/rfc/rfc7232#page-18 suggests Last-Modified should
                                    // still be included in the 304 response
                                    let response =
                                        Response::empty(NOT_MODIFIED).with_header(Header {
                                            field: last_modified_field.clone(),
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
                            let mut response =
                                Response::from_file(file).with_header(atom_content_type.clone());
                            if let Some(modified) = modified {
                                response = response.with_header(Header {
                                    field: last_modified_field.clone(),
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
                (Method::Post, "/add") => {
                    let _lock = self.feed_lock.write().expect("poisioned");
                    match self.add(&mut request) {
                        Ok(()) => Response::from_string("Added\n").with_status_code(CREATED),
                        Err(StatusError(status, error)) => {
                            Response::from_string(format!("Failed: {error}\n"))
                                .with_status_code(status)
                        }
                    }
                }
                _ => Response::from_string(embed!("404.html"))
                    .with_header(html_content_type.clone())
                    .with_status_code(NOT_FOUND),
            };

            self.log_request(&request, response.status_code());

            match request.respond(response) {
                Ok(()) => {}
                Err(err) => error!("Failed to send response: {err}"),
            }
        }
    }

    fn add(&self, request: &mut Request) -> Result<(), StatusError> {
        self.validate_request(request)?;

        // Get the text field of the form data
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
        if page.title.is_none() && title.is_some() {
            page.title = title.map(|cow| cow.into_owned());
        }

        // Add to the feed
        let mut feed = Feed::read(&self.feed_path).map_err(|err| {
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

    fn validate_request(&self, request: &Request) -> Result<(), StatusError> {
        // Extract required headers
        let content_type = request
            .headers()
            .iter()
            .find(|&header| header.field == self.content_type_field)
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
                (header.field == self.user_agent_field).then(|| header.value.as_str())
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

impl StatusError {
    fn new<C: Into<StatusCode>>(code: C, message: &'static str) -> Self {
        StatusError(code.into(), message)
    }
}
