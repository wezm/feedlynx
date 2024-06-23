use std::{fmt, io};

use html5gum::{HtmlString, IoReader, Tokenizer};
use log::{log_enabled, trace};
use minreq::URL;

#[derive(Default)]
pub struct WebPage {
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug)]
pub enum WebPageError {
    Http(minreq::Error),
    Io(io::Error),
    Unsuccessful {
        status_code: i32,
        reason_phrase: String,
    },
}

pub fn fetch<U: Into<URL>>(url: U) -> Result<WebPage, WebPageError> {
    let resp = minreq::get(url)
        .with_timeout(30)
        .with_max_redirects(10)
        .with_max_headers_size(4096)
        .with_max_status_line_length(1024)
        // DuckDuckBot/1.1; (+http://duckduckgo.com/duckduckbot.html)
        .with_header(
            "User-Agent",
            format!(
                "{}/{}; (+{})",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_HOMEPAGE"),
            ),
        )
        .send_lazy()?;

    if resp.status_code != 200 {
        return Err(WebPageError::Unsuccessful {
            status_code: resp.status_code,
            reason_phrase: resp.reason_phrase,
        });
    }

    let tokenizer = Tokenizer::new(IoReader::new(resp));

    extract_meta_data(tokenizer)
}

fn extract_meta_data(
    tokenizer: Tokenizer<IoReader<minreq::ResponseLazy>>,
) -> Result<WebPage, WebPageError> {
    let mut title = None;
    let mut description = None;

    let property_attr = HtmlString(b"property".to_vec());
    let content_attr = HtmlString(b"content".to_vec());
    let name_attr = HtmlString(b"name".to_vec());
    let description_attr = HtmlString(b"description".to_vec());

    let mut title_tag = String::new();
    let mut in_title = false;
    for token in tokenizer {
        let token = token?; // TODO: If we already have a title or description when hitting an error then maybe just return what we have so far

        match token {
            // <meta>
            html5gum::Token::StartTag(tag) if *tag.name == b"meta" => {
                trace!("Tag {:?}", tag);
                let content = tag
                    .attributes
                    .get(&content_attr)
                    .and_then(|v| std::str::from_utf8(v).ok());
                let Some(content) = content.map(str::trim) else {
                    // If content isn't present then no point checking the other stuff
                    trace!("content missing or invalid");
                    continue;
                };

                let property = tag.attributes.get(&property_attr);
                match property.map(|v| v.as_slice()) {
                    Some(b"og:title") => set_if_longer(&mut title, content),
                    Some(b"og:description") => set_if_longer(&mut description, content),
                    Some(_) => {}
                    // Check for <meta name="description" content="...">
                    None => {
                        let name = tag.attributes.get(&name_attr);
                        if name == Some(&description_attr) {
                            set_if_longer(&mut description, content)
                        }
                    }
                }
            }
            // <title>
            html5gum::Token::StartTag(tag) if *tag.name == b"title" => in_title = true,
            html5gum::Token::EndTag(tag) if *tag.name == b"title" => {
                in_title = false;
            }
            html5gum::Token::String(text) if in_title => {
                if let Ok(text) = std::str::from_utf8(&text) {
                    title_tag.push_str(text);
                }
            }
            _ => {}
        }
    }

    if title.is_none() && !title_tag.trim().is_empty() {
        set_if_longer(&mut title, &title_tag)
    }

    Ok(WebPage { title, description })
}

fn set_if_longer(value: &mut Option<String>, candidate: &str) {
    match value {
        Some(existing) if candidate.len() > existing.len() => {
            value.replace(candidate.to_string());
        }
        Some(_) => {}
        None => *value = Some(candidate.to_string()),
    }
}

impl From<minreq::Error> for WebPageError {
    fn from(err: minreq::Error) -> Self {
        WebPageError::Http(err)
    }
}

impl From<io::Error> for WebPageError {
    fn from(err: io::Error) -> Self {
        WebPageError::Io(err)
    }
}

impl fmt::Display for WebPageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WebPageError::Http(err) => write!(f, "HTTP error: {err}"),
            WebPageError::Io(err) => write!(f, "I/O error: {err}"),
            WebPageError::Unsuccessful {
                status_code,
                reason_phrase,
            } => write!(
                f,
                "HTTP request was unsuccessful: {reason_phrase} ({status_code})"
            ),
        }
    }
}

impl std::error::Error for WebPageError {}
