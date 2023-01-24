use chrono::{Days, NaiveDate, Utc};
use clap::Parser;
use notify_rust::Notification;
use rand::distributions::{Alphanumeric, DistString};
use sqlite::State;
use std::{
    fs::{self, File},
    io::{ErrorKind, Read, Seek, Write},
    path::Path,
    process::{Child, Command},
    str,
    time::Duration,
};
use thirtyfour::{
    common::capabilities::firefox::FirefoxPreferences, cookie::time::OffsetDateTime, prelude::*,
    support::sleep,
};

struct GeckoDriver {
    child: Child,
}

impl GeckoDriver {
    fn start(gecko_driver: String) -> GeckoDriver {
        let mut command = Command::new(gecko_driver);
        let child = command.spawn().expect("Failed to start driver");

        GeckoDriver { child }
    }
}

impl Drop for GeckoDriver {
    fn drop(&mut self) {
        println!("killing child");
        self.child.kill().expect("Wasn't running");
    }
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
struct Cli {
    /// Gecko driver path
    #[arg(long, short)]
    gecko_driver: String,

    /// Firefox path if not installed in default folder
    #[arg(long, short)]
    firefox: Option<String>,

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

    let _gecko_driver = GeckoDriver::start(cli.gecko_driver);

    let cookies = cli.profile + r"\cookies.sqlite";

    let result = run_requests(cookies, cli.firefox);
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
            Notification::new()
                .summary("Reward bing")
                .body(&format!("Bing reward failed: {}", e.to_string()))
                .show()
                .unwrap();
        }
    };
}

#[tokio::main]
async fn run_requests(
    firefox_cookies: String,
    firefox_path: Option<String>,
) -> WebDriverResult<()> {
    let edge_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/64.0.3282.140 Safari/537.36 Edge/18.17763".to_owned();
    let android_agent =
        "Mozilla/5.0 (Android 11; Mobile; rv:83.0) Gecko/83.0 Firefox/83.0".to_owned();

    let cookies = get_firefox_cookies(firefox_cookies);

    search_with_user_agent(&firefox_path, &cookies, edge_agent, 1).await?; // 40
    search_with_user_agent(&firefox_path, &cookies, android_agent, 1).await?; // 25

    Ok(())
}

async fn search_with_user_agent(
    firefox_path: &Option<String>,
    cookies: &Vec<Cookie<'static>>,
    user_agent: String,
    request_number: i32,
) -> WebDriverResult<()> {
    let mut caps = DesiredCapabilities::firefox();

    if let Some(firefox_path) = firefox_path {
        caps.add_firefox_option("binary", firefox_path)?;
    }

    caps.set_headless()?;

    let mut firefox_preference = FirefoxPreferences::new();
    firefox_preference.set_user_agent(user_agent)?;
    caps.set_preferences(firefox_preference)?;

    let driver = WebDriver::new("http://127.0.0.1:4444", caps.clone()).await?;

    driver.goto("https://bing.com").await?;
    for cookie in cookies {
        driver.add_cookie(cookie.clone()).await?;
    }

    for _ in 0..request_number {
        sleep(Duration::from_secs(1)).await;
        let mut random_url = "https://bing.com/search?q=".to_owned();
        random_url.push_str(&Alphanumeric.sample_string(&mut rand::thread_rng(), 16));

        driver.goto(random_url).await?;
    }

    driver.quit().await?;

    Ok(())
}

fn get_firefox_cookies(cookie_file: String) -> Vec<Cookie<'static>> {
    let connection = sqlite::open(cookie_file).expect("db Connection failed");

    let query =
        "SELECT * FROM moz_cookies WHERE (host = '.bing.com' OR host = 'www.bing.com') AND value != '' AND originAttributes = ''";
    let mut statement = connection.prepare(query).unwrap();

    let mut cookies = Vec::new();
    while let Ok(State::Row) = statement.next() {
        let cookie = Cookie::build(
            statement.read::<String, _>("name").unwrap(),
            statement.read::<String, _>("value").unwrap(),
        )
        .domain(statement.read::<String, _>("host").unwrap())
        .expires(
            OffsetDateTime::from_unix_timestamp(statement.read::<i64, _>("expiry").unwrap())
                .unwrap(),
        )
        .path(statement.read::<String, _>("path").unwrap())
        .http_only(statement.read::<i64, _>("isHttpOnly").unwrap() != 0)
        .finish();

        cookies.push(cookie);
    }

    cookies
}
