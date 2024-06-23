use std::borrow::Cow;
use std::error::Error;
use std::fs::File;
use std::net::ToSocketAddrs;
use std::path::PathBuf;

use log::{debug, error, info, log_enabled, warn};
use tiny_http::{Header, HeaderField, Method, Request, Response, StatusCode};
use uriparse::URI;

use crate::feed::Feed;
use crate::webpage::WebPage;
use crate::{embed, webpage, FeedToken, PrivateToken};

pub struct Server {
    server: tiny_http::Server,
    private_token: PrivateToken,
    feed_path: PathBuf,
    feed_route: String,
    content_type_field: HeaderField,
}

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
            content_type_field: "Content-Type".parse().unwrap(),
        })
    }

    pub fn handle_requests(&self) {
        let html_content_type: Header = "Content-type: text/html; charset=utf-8".parse().unwrap();
        let atom_content_type: Header = "Content-type: application/atom+xml".parse().unwrap();

        info!("feed available at {}", self.feed_route);

        for mut request in self.server.incoming_requests() {
            let response = match (request.method(), request.url()) {
                (Method::Get, "/") => Response::from_string(embed!("index.html"))
                    .with_header(html_content_type.clone()),
                // TODO: Handle query args (I.e. ignore them?)
                (Method::Get, path) if path == self.feed_route => {
                    match File::open(&self.feed_path) {
                        Ok(file) => {
                            // TODO: Set cache headers on the response
                            // TODO: Handle cache headers on the request

                            // This branch has a different response type so we have to call respond and continue
                            // instead of falling through to the code at the bottom.
                            let response =
                                Response::from_file(file).with_header(atom_content_type.clone());
                            log_request(&request, response.status_code());
                            match request.respond(response) {
                                Ok(()) => {}
                                Err(err) => error!("Failed to send response: {err}"),
                            }
                            continue;
                        }
                        Err(err) => {
                            error!("unable to open feed file: {}", err);
                            Response::from_string(embed!("500.html")).with_status_code(500)
                        }
                    }
                }
                (Method::Post, "/add") => match self.add(&mut request) {
                    Ok(()) => Response::from_string("Added\n").with_status_code(201),
                    Err(status) => Response::from_string("Failed").with_status_code(status),
                },
                _ => Response::from_string(embed!("404.html"))
                    .with_header(html_content_type.clone())
                    .with_status_code(404),
            };

            log_request(&request, response.status_code());

            match request.respond(response) {
                Ok(()) => {}
                Err(err) => error!("Failed to send response: {err}"),
            }
        }
    }

    fn add(&self, request: &mut Request) -> Result<(), StatusCode> {
        self.validate_request(request)?;

        // Get the text field of the form data
        // FIXME: Limit the size of the body that will be read
        let mut body = Vec::new();
        if request.as_reader().read_to_end(&mut body).is_err() {
            return Err(StatusCode::from(500));
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

        let token = token.ok_or(StatusCode::from(400))?;

        // Validate token
        if self.private_token != *token {
            return Err(StatusCode::from(401)); // TODO: constant these codes
        }

        // Parse URL
        let Some(url) = url.as_ref().and_then(|u| URI::try_from(u.as_ref()).ok()) else {
            return Err(StatusCode::from(400)); // Bad request
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
        let mut feed = match Feed::read(&self.feed_path) {
            Ok(feed) => feed,
            Err(err) => {
                error!("Unable to read feed: {err}");
                return Err(StatusCode::from(500));
            }
        };
        feed.add_url(&url, page);
        feed.trim_entries();
        match feed.save() {
            Ok(()) => Ok(()),
            Err(err) => {
                error!("Unable to save feed: {err}");
                Err(StatusCode::from(500))
            }
        }
    }

    fn validate_request(&self, request: &Request) -> Result<(), StatusCode> {
        const BAD_REQUEST: u16 = 400;

        // Extract required headers
        let content_type = request
            .headers()
            .iter()
            .find(|&header| header.field == self.content_type_field)
            .ok_or_else(|| StatusCode::from(BAD_REQUEST))?;

        if content_type.value != "application/x-www-form-urlencoded" {
            return Err(StatusCode::from(400));
        }

        Ok(())
    }
}

fn is_blank(text: &str) -> bool {
    text.chars().all(|ch| ch.is_whitespace())
}

fn log_request(request: &Request, status: StatusCode) {
    if log_enabled!(log::Level::Debug) {
        let user_agent_header: HeaderField = "User-Agent".parse().unwrap(); // TODO: avoid parsing this every time
        let host = request
            .remote_addr()
            .map(|sock| Cow::from(sock.to_string()))
            .unwrap_or_else(|| Cow::from("-"));
        let user_agent = request.headers().iter().find_map(|header| {
            if header.field == user_agent_header {
                Some(header.value.as_str())
            } else {
                None
            }
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
