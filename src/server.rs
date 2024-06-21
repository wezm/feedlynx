use std::error::Error;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};

use tiny_http::{Header, HeaderField, Method, Request, Response, StatusCode};
use uriparse::URI;

macro_rules! embed {
    ($path:literal) => {{
        #[cfg(debug_assertions)]
        {
            use std::{borrow::Cow, fs, path::Path};

            let data = Path::new(file!())
                .parent()
                .ok_or_else(|| "no parent".to_string())
                .map(|parent| parent.join($path))
                .and_then(|path| fs::read_to_string(&path).map_err(|err| err.to_string()))
                .map(Cow::<'static, str>::Owned);
            match data {
                Ok(data) => data,
                Err(err) => panic!("unable to embed {}: {}", $path, err),
            }
        }
        #[cfg(not(debug_assertions))]
        {
            use std::borrow::Cow;

            Cow::<'static, str>::Borrowed(include_str!($path));
        }
    }};
}

pub struct Server {
    server: tiny_http::Server,
    //status: Arc<Mutex<DeviceStatuses>>,
}

impl Server {
    pub fn new<A>(
        addr: A,
        //status: Arc<Mutex<DeviceStatuses>>,
    ) -> Result<Server, Box<dyn Error + Send + Sync + 'static>>
    where
        A: ToSocketAddrs,
    {
        tiny_http::Server::http(addr).map(|server| Server { server })
    }

    pub fn handle_requests(&self) {
        let HTML_CONTENT_TYPE: Header = "Content-type: text/html; charset=utf-8".parse().unwrap();
        let CSS_CONTENT_TYPE: Header = "Content-type: text/css; charset=utf-8".parse().unwrap();
        let SVG_CONTENT_TYPE: Header = "Content-type: image/svg+xml".parse().unwrap();
        let JS_CONTENT_TYPE: Header = "Content-type: text/javascript; charset=utf-8"
            .parse()
            .unwrap();
        let JSON_CONTENT_TYPE: Header = "Content-type: application/json".parse().unwrap();

        for mut request in self.server.incoming_requests() {
            let response = match (request.method(), request.url()) {
                // TODO: Require GET, handle HEAD
                (Method::Get, "/") => Response::from_string(embed!("index.html"))
                    .with_header(HTML_CONTENT_TYPE.clone()),
                (Method::Get, "/status.json") => {
                    //let status = self.status.lock().expect("poisioned");
                    //let asdf = status
                    //    .iter()
                    //    .map(|s| DeviceWithStatus::from(s))
                    //    .collect::<Vec<_>>();
                    //Response::from_string(serde_json::to_string(&asdf).unwrap())
                    //    .with_header(JSON_CONTENT_TYPE.clone())
                    todo!()
                }
                //"/wattsmart.js" => Response::from_string(embed!("../gleam/dist/wattsmart.js"))
                //    .with_header(JS_CONTENT_TYPE.clone()),
                //"/style.css" => {
                //    Response::from_string(embed!("style.css")).with_header(CSS_CONTENT_TYPE.clone())
                //}
                //path if path.starts_with("/images/") => {
                //    let data = match path {
                //        "/images/computer.svg" => Some(embed!("noun-desktop-computer-6623437.svg")),
                //        "/images/dishwasher.svg" => Some(embed!("noun-dishwasher-6745009.svg")),
                //        "/images/freezer.svg" => Some(embed!("noun-freezer-4185404.svg")),
                //        "/images/heater.svg" => Some(embed!("noun-heater-4214680.svg")),
                //        "/images/lamp-tall.svg" => Some(embed!("noun-lamp-6798375.svg")),
                //        "/images/lamp.svg" => Some(embed!("noun-lamp-6826738.svg")),
                //        "/images/tv.svg" => Some(embed!("noun-lcd-37373.svg")),
                //        "/images/washing-machine.svg" => {
                //            Some(embed!("noun-washing-machine-6507851.svg"))
                //        }
                //        "/images/unknown.svg" => Some(embed!("noun-sensor-3980567.svg")),
                //
                //        "/images/power-on.svg" => Some(embed!("noun-power-on-6638635.svg")),
                //        "/images/power-off.svg" => Some(embed!("noun-power-on-6638664.svg")),
                //        _ => None,
                //    };
                //
                //    match data {
                //        Some(data) => {
                //            Response::from_string(data).with_header(SVG_CONTENT_TYPE.clone())
                //        }
                //        None => Response::from_string(embed!("404.html"))
                //            .with_header(HTML_CONTENT_TYPE.clone())
                //            .with_status_code(404),
                //    }
                //}
                _ => Response::from_string(embed!("404.html"))
                    .with_header(HTML_CONTENT_TYPE.clone())
                    .with_status_code(404),
            };

            // Ignoring I/O errors that occur here so that we don't take down the process if there
            // is an issue sending the response.
            let _ = request.respond(response);
        }
    }

    fn add(&self, request: &mut Request) -> StatusCode {
        let content_type = match Self::validate_request(request) {
            Ok(headers) => headers,
            Err(status) => {
                return status;
            }
        };

        // Get the text field of the form data
        // FIXME: Limit the size of the body that will be read
        let mut body = Vec::new();
        if request.as_reader().read_to_end(&mut body).is_err() {
            return (StatusCode::from(500));
        }

        // Parse the form submission and extract the token and url
        let mut token = None;
        let mut url = None;

        form_urlencoded::parse(&body).for_each(|(key, value)| match &*key {
            "token" => token = Some(value),
            "url" => url = Some(value),
            _ => {}
        });

        // Validate token
        //if token != self.token {
        //    return StatusCode::from(401); // TODO: constant these codes
        //}

        // Parse URL
        let Some(url) = url.as_ref().map(|u| URI::try_from(u.as_ref()).ok()) else {
            return StatusCode::from(400); // Bad request
        };

        //find(|(key, _value)| key == "text") {
        //    Some((_key, text)) if !is_blank(&text) => (
        //        object! {
        //          "response_type": "in_channel",
        //          "text": &*substitute_urls(&text),
        //        },
        //        StatusCode::from(200),
        //    ),
        //    Some(_) | None => (
        //        object! {
        //            "response_type": "ephemeral",
        //            "text": "You need to supply some text",
        //        },
        //        StatusCode::from(200),
        //    ),
        //}

        StatusCode::from(201)
    }

    fn validate_request(request: &Request) -> Result<(), StatusCode> {
        const BAD_REQUEST: u16 = 400;
        let CONTENT_TYPE: HeaderField = "Content-Type".parse().unwrap();

        // Extract required headers
        let content_type = request
            .headers()
            .iter()
            .find(|&header| header.field == CONTENT_TYPE)
            .ok_or_else(|| {
                //String::from("Content-Type header not found"),
                StatusCode::from(BAD_REQUEST)
            })?;

        if content_type.value != "application/x-www-form-urlencoded" {
            return Err(StatusCode::from(400));
        }
        //let authorization = request
        //    .headers()
        //    .iter()
        //    .find(|&header| header.field == *AUTHORIZATION)
        //    .ok_or_else(|| {
        //        (
        //            String::from("Authorization header not found"),
        //            StatusCode::from(BAD_REQUEST),
        //        )
        //    })?;
        Ok(())
    }
}

fn is_blank(text: &str) -> bool {
    text.chars().all(|ch| ch.is_whitespace())
}

/*

<iframe width="560" height="315" src="https://www.youtube.com/embed/1162ouPHH3Q?si=NxxME0UqCBVlCsQK" title="YouTube video player" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share" referrerpolicy="strict-origin-when-cross-origin" allowfullscreen></iframe>

 <entry>
  <id>yt:video:1162ouPHH3Q</id>
  <yt:videoId>1162ouPHH3Q</yt:videoId>
  <yt:channelId>UCJYJgj7rzsn0vdR7fkgjuIA</yt:channelId>
  <title>I'm sick in a bizarre and horrifying way</title>
  <link rel="alternate" href="https://www.youtube.com/watch?v=1162ouPHH3Q"/>
  <author>
   <name>styropyro</name>
   <uri>https://www.youtube.com/channel/UCJYJgj7rzsn0vdR7fkgjuIA</uri>
  </author>
  <published>2024-06-06T18:28:02+00:00</published>
  <updated>2024-06-13T20:36:43+00:00</updated>
  <media:group>
   <media:title>I'm sick in a bizarre and horrifying way</media:title>
   <media:content url="https://www.youtube.com/v/1162ouPHH3Q?version=3" type="application/x-shockwave-flash" width="640" height="390"/>
   <media:thumbnail url="https://i2.ytimg.com/vi/1162ouPHH3Q/hqdefault.jpg" width="480" height="360"/>
   <media:description>my crazy tornado hunting adventure: https://www.youtube.com/watch?v=qR4p6knJuus

links:
storm chasing channel: https://www.youtube.com/@styro_drake
shorts channel: https://www.youtube.com/@styropyroshorts
instagram: https://www.instagram.com/styro.drake/
patreon: https://www.patreon.com/styropyro
twitter: https://twitter.com/styropyro_
discord: https://discord.gg/hVZMcWT</media:description>
   <media:community>
    <media:starRating count="193523" average="5.00" min="1" max="5"/>
    <media:statistics views="2047516"/>
   </media:community>
  </media:group>
 </entry>

*/
