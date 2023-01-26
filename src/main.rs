use chrono::{Days, NaiveDate, Utc};
use clap::Parser;
use notify_rust::Notification;
use rand::distributions::{Alphanumeric, DistString};
use reqwest::{cookie::Jar, Client, Url};
use sqlite::{State, Statement};
use std::{
    error::Error,
    fs::{self, File},
    io::{ErrorKind, Read, Seek, Write},
    path::Path,
    str,
    sync::Arc,
    time::Duration,
};
use tokio::time::sleep;

#[derive(Parser)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
struct Cli {
    /// Firefox profile folder path to get cookies from (Win + R => "firefox -P" to see profiles then hover to see folder)
    #[arg(long, short)]
    profile: String,
}

fn main() {
    let cli = Cli::parse();

    let binding = dirs::data_dir()
        .unwrap()
        .join(Path::new(r"BingReward\last-date.txt"));
    let data_file_path = binding.as_path();

    let mut file = File::options()
        .read(true)
        .write(true)
        .create(false)
        .open(data_file_path)
        .unwrap_or_else(|e| {
            if e.kind() == ErrorKind::NotFound {
                fs::create_dir_all(data_file_path.parent().unwrap()).unwrap();

                File::options()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(data_file_path)
                    .unwrap()
            } else {
                panic!("Error when creating file");
            }
        });

    let mut contents = vec![];
    file.read_to_end(&mut contents)
        .expect("Fail reading data file");

    let date_string = str::from_utf8(&contents).expect("Not utf8");
    let last_date = if date_string.is_empty() {
        Utc::now().date_naive() - Days::new(1)
    } else {
        date_string
            .parse::<NaiveDate>()
            .expect("Could not parse date")
    };

    let now = Utc::now().date_naive();

    if now <= last_date {
        return;
    }

    let cookies_file = cli.profile + r"\cookies.sqlite";

    let result = run_requests(cookies_file);
    match result {
        Ok(_) => {
            Notification::new()
                .summary("Reward bing")
                .body("Bing reward completed successfully.")
                .show()
                .unwrap();

            file.seek(std::io::SeekFrom::Start(0)).unwrap();
            file.write_all(now.to_string().as_bytes())
                .expect("Failed to write");
        }
        Err(e) => {
            let error = format!("Bing reward failed: {}", e.to_string());

            Notification::new()
                .summary("Reward bing")
                .body(&error)
                .show()
                .unwrap();
        }
    };
}

#[tokio::main(flavor = "current_thread")]
async fn run_requests(firefox_cookies: String) -> Result<(), Box<dyn Error>> {
    let edge_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/64.0.3282.140 Safari/537.36 Edge/18.17763".to_owned();
    let android_agent =
        "Mozilla/5.0 (Android 11; Mobile; rv:83.0) Gecko/83.0 Firefox/83.0".to_owned();

    let cookies = get_firefox_cookies(firefox_cookies)?;

    search_with_user_agent(&cookies, &edge_agent, 40).await?; // 40
    search_with_user_agent(&cookies, &android_agent, 25).await?; // 25

    Ok(())
}

async fn search_with_user_agent(
    cookies: &String,
    user_agent: &String,
    request_number: i32,
) -> Result<(), Box<dyn Error>> {
    let cookie_store = Jar::default();
    let url = "https://www.bing.com".parse::<Url>().unwrap();
    cookie_store.add_cookie_str(cookies, &url);

    let cookie_store = Arc::new(cookie_store);

    let client = Client::builder()
        .user_agent(user_agent)
        .cookie_provider(Arc::clone(&cookie_store))
        .build()?;

    for _ in 0..request_number {
        sleep(Duration::from_secs(1)).await;
        let mut random_url = "https://bing.com/search?q=".to_owned();
        random_url.push_str(&Alphanumeric.sample_string(&mut rand::thread_rng(), 16));

        let _response = client.get(random_url).send().await?;
    }

    Ok(())
}

fn get_firefox_cookies(cookie_file: String) -> Result<std::string::String, sqlite::Error> {
    let connection = sqlite::open(cookie_file).expect("db Connection failed");

    let query =
        "SELECT * FROM moz_cookies WHERE (host = '.bing.com' OR host = 'www.bing.com') AND value != '' AND originAttributes = '' GROUP BY name;";
    let mut statement = connection.prepare(query).unwrap();

    let mut cookies = Vec::new();
    while let Ok(State::Row) = statement.next() {
        let pair = retrieve_value(&mut statement)?;

        cookies.push(pair);
    }

    Ok(cookies.join("; "))
}

fn retrieve_value(statement: &mut Statement) -> Result<std::string::String, sqlite::Error> {
    let name = statement.read::<String, _>("name")?;
    let value = statement.read::<String, _>("value")?;
    Ok(format!("{name}={value}"))
}
