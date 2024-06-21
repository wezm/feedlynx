use std::fs::File;
use std::io::BufReader;
use std::time::SystemTime;
use std::{env, process};

use atom_syndication::{Feed, FixedDateTime, Generator};
use chrono::{DateTime, FixedOffset, Utc};

use vidlater::Server;

fn main() {
    // Read
    let file = File::open("src/default.xml").unwrap();
    let mut feed = Feed::read_from(BufReader::new(file)).unwrap();

    // Write
    let generator = Generator {
        value: env!("CARGO_PKG_NAME").to_string(),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        ..Default::default()
    };
    feed.set_generator(generator);

    //let sys_time = SystemTime::now();
    //let timestamp=sys_time.duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let local: DateTime<Utc> = Utc::now();

    //let now = FixedDateTime::from_timestamp(timestamp, 0).unwrap();
    feed.set_updated(local);

    // write to the feed to a writer
    //feed.write_to(sink()).unwrap();

    // convert the feed to a string
    let string = feed.to_string();
    println!("{}", string);

    let server_addr = (
        env::var("VIDLATER_ADDRESS").unwrap_or_else(|_| String::from("0.0.0.0")),
        env::var("VIDLATER_PORT")
            .ok()
            .and_then(|port| port.parse::<u16>().ok())
            .unwrap_or(8001),
    );

    let server = match Server::new(server_addr.clone()) {
        Ok(server) => server,
        Err(err) => {
            eprintln!(
                "ERROR: Unable to start http server on {}:{}: {}",
                server_addr.0, server_addr.1, err
            );
            process::exit(1);
        }
    };

    println!(
        "INFO: http server running on http://{}:{}",
        server_addr.0, server_addr.1
    );
    server.handle_requests();
}
