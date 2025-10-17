use wreq::Client;
use wreq::header::{HeaderMap, HeaderValue, USER_AGENT, COOKIE};
use wreq_util::Emulation;
use scraper::{Html, Selector};
use serde::{Deserialize, Deserializer};
use std::fs::File;
use std::io::{BufReader, Write};
use regex::Regex;
use std::time::Instant;
use argh::FromArgs;
use std::ops::RangeInclusive;
use rusqlite::{Connection, params};
use tokio::{join, task};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use indicatif::{ProgressBar, ProgressStyle};

const FAV_URL: &str = "https://nhentai.net/favorites/";
const GALLERY_URL: &str = "https://nhentai.net/api/gallery/";
const SQ_DB_FILE: &str = "nfavs.db";

#[derive(Deserialize)]
struct Config {
    user_agent: String,
    cookies: Vec<String>,
}

#[derive(Debug)]
struct HentaiBook {
    url: String,
    thumbnail_url: String,
}

#[derive(Debug)]
struct BookData {
    title_1: String,
    title_2: String,
    artists: String,
    groups: String,
    tags: String,
    languages: String,
    id: String,
    pages: u32,
}

// == nhentai gallery api json structures ==
#[derive(Debug, Deserialize)]
struct NhentaiTitle {
    english: Option<String>,
    japanese: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NhentaiTag {
    #[serde(rename = "type")]
    tag_type: String,
    name: String,
    count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NhentaiResponse {
    Gallery {
        #[serde(deserialize_with = "deserialize_string_or_number")]
        id: u32,
        title: NhentaiTitle,
        tags: Vec<NhentaiTag>,
        num_pages: u32,
    },
    Error {
        error: String,
    },
}
//==============================

#[derive(Debug)]
struct UserData {
    user_name: String,
    page_count: u32,
    fav_count: u32,
}

#[derive(Debug, Clone)]
struct Stats {
    total_skipped: Arc<AtomicU32>,
    total_added: Arc<AtomicU32>,
}

impl Stats {
    fn new() -> Self {
        Self {
            total_skipped: Arc::new(AtomicU32::new(0)),
            total_added: Arc::new(AtomicU32::new(0)),
        }
    }

    fn inc_skipped(&self) {
        self.total_skipped.fetch_add(1, Ordering::SeqCst);
    }

    fn inc_added(&self) {
        self.total_added.fetch_add(1, Ordering::SeqCst);
    }

    fn get_skipped(&self) -> u32 {
        self.total_skipped.load(Ordering::SeqCst)
    }

    fn get_added(&self) -> u32 {
        self.total_added.load(Ordering::SeqCst)
    }
}

fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(u32),
    }

    match StringOrNumber::deserialize(deserializer)? {
        StringOrNumber::String(s) => s.parse().map_err(|_| {
            serde::de::Error::custom(format!("Failed to parse '{}' as u32", s))
        }),
        StringOrNumber::Number(n) => Ok(n),
    }
}

//=========== Response parsers ========
fn parse_login_data(html: &str) -> Result<UserData, String> {
    let document = Html::parse_document(html);

    let username_selector = Selector::parse("span.username").map_err(|e| e.to_string())?;
    let count_selector = Selector::parse("span.count").map_err(|e| e.to_string())?;
    let lastpage_selector = Selector::parse("a.last").map_err(|e| e.to_string())?;

    let user_name = document
        .select(&username_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Login failed: username not found".to_string())?;

    let fav_count = document
        .select(&count_selector)
        .next()
        .and_then(|el| {
            let txt = el.text().collect::<String>();
            let cleaned = txt
                .replace(['(', ')', ','], "")
                .trim()
                .to_string();
            cleaned.parse::<u32>().ok()
        })
        .unwrap_or(0);

    let page_count = document
        .select(&lastpage_selector)
        .next()
        .and_then(|el| el.value().attr("href"))
        .and_then(|href| href.split("page=").nth(1)?.parse::<u32>().ok())
        .unwrap_or_else(|| {
            if fav_count > 0 {
                1
            } else {
                0
            }
        });

    Ok(UserData {
        user_name,
        page_count,
        fav_count,
    })
}

fn parse_fav_page(html: &str) -> Result<Vec<HentaiBook>, String> {
    let document = Html::parse_document(html);

    let gallery_selector = Selector::parse("div.gallery-favorite").map_err(|e| e.to_string())?;
    let img_selector = Selector::parse("img.lazyload").map_err(|e| e.to_string())?;
    let link_selector = Selector::parse("a.cover").map_err(|e| e.to_string())?;

    let mut books = Vec::new();

    for gallery_div in document.select(&gallery_selector) {
        let thumbnail_url = gallery_div
            .select(&img_selector)
            .next()
            .and_then(|img| img.value().attr("data-src"))
            .map(|src| format!("https:{}", src))
            .unwrap_or_default();

        let url = gallery_div
            .select(&link_selector)
            .next()
            .and_then(|a| a.value().attr("href"))
            .and_then(|href| extract_id_from_url(href))
            .map(|id| format!("{}{}", GALLERY_URL, id))
            .unwrap_or_default();

        if !url.is_empty() && !thumbnail_url.is_empty() {
            books.push(HentaiBook { url, thumbnail_url });
        } else {
            eprintln!("[-] Skipping gallery due to missing fields: url='{}', thumbnail='{}'",
                url, thumbnail_url
            );
        }
    }

    Ok(books)
}

fn parse_book_json(json_str: &str) -> Result<BookData, String> {
    let parsed: NhentaiResponse =
        serde_json::from_str(json_str).map_err(|e| format!("Invalid JSON: {e}"))?;

    match parsed {
        NhentaiResponse::Error { error } => Err(format!("Gallery error: {error}")),

        NhentaiResponse::Gallery {
            id,
            title,
            tags,
            num_pages,
        } => {
            let mut artists = Vec::new();
            let mut groups = Vec::new();
            let mut tags_vec = Vec::new();
            let mut languages = Vec::new();

            for tag in tags {
                match tag.tag_type.as_str() {
                    "artist" => artists.push(tag),
                    "group" => groups.push(tag),
                    "tag" => tags_vec.push(tag),
                    "language" => languages.push(tag),
                    _ => {}
                }
            }

            // Sort only by popularity (count descending)
            let sort_tags = |mut v: Vec<NhentaiTag>| -> Vec<String> {
                v.sort_by(|a, b| b.count.cmp(&a.count));
                v.into_iter().map(|t| t.name).collect()
            };

            Ok(BookData {
                title_1: title.english.unwrap_or_default(),
                title_2: title.japanese.unwrap_or_default(),
                artists: sort_tags(artists).join(", "),
                groups: sort_tags(groups).join(", "),
                tags: sort_tags(tags_vec).join(", "),
                languages: sort_tags(languages).join(", "),
                id: id.to_string(),
                pages: num_pages,
            })
        }
    }
}

fn extract_id_from_url(url: &str) -> Option<String> {
    let re = Regex::new(r"(?:/g/|/gallery/)(\d+)/?").unwrap();
    re.captures(url)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
}

fn extract_image_extension(url: &str) -> String {
    url.split('.')
        .last()
        .and_then(|ext| ext.split('?').next())
        .unwrap_or("jpg")
        .to_string()
}

// =================Database Stuff==========
fn init_db(db_path: &Path) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS books (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            nhen_id TEXT UNIQUE,
            title_1 TEXT,
            title_2 TEXT,
            artists TEXT,
            groups TEXT,
            tags TEXT,
            languages TEXT,
            pages INTEGER,
            image_ext TEXT
        )",
        [],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn get_total_books_in_db(db_path: &Path) -> Result<u32, String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM books").map_err(|e| e.to_string())?;
    let count: u32 = stmt.query_row([], |row| row.get(0)).map_err(|e| e.to_string())?;
    Ok(count)
}

fn export_db_to_csv(db_path: &Path, out_path: &Path) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("
        SELECT nhen_id, title_1, title_2, artists, groups, tags, languages, pages
        FROM books ORDER BY id DESC
	").map_err(|e| e.to_string())?;

    let books = stmt.query_map([], |row| {
        Ok(BookData {
            id: row.get(0)?,
            title_1: row.get(1)?,
            title_2: row.get(2)?,
            artists: row.get(3)?,
            groups: row.get(4)?,
            tags: row.get(5)?,
            languages: row.get(6)?,
            pages: row.get(7)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut wtr = csv::Writer::from_path(out_path).map_err(|e| e.to_string())?;
    wtr.write_record(&[
        "nhen_id",
        "title_1",
        "title_2",
        "artists",
        "groups",
        "tags",
        "languages",
        "pages",
    ]).map_err(|e| e.to_string())?;

    for book_result in books {
        let book = book_result.map_err(|e| e.to_string())?;
        wtr.write_record(&[
            book.id.to_string(),
            book.title_1,
            book.title_2,
            book.artists,
            book.groups,
            book.tags,
            book.languages,
            book.pages.to_string(),
        ]).map_err(|e| e.to_string())?;
    }

    wtr.flush().map_err(|e| e.to_string())?;
    println!("[+] Exported database to {}", out_path.display());
    Ok(())
}

fn book_exists_in_db(conn: &Connection, id: &str) -> Result<bool, String> {
    let mut stmt = conn.prepare("SELECT nhen_id FROM books WHERE nhen_id = ?1").map_err(|e| e.to_string())?;
    let exists = stmt.exists(params![id]).map_err(|e| e.to_string())?;
    Ok(exists)
}

fn save_book_to_db(conn: &Connection, book_data: &BookData, ext: &str) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO books (nhen_id, title_1, title_2, artists, groups, tags, languages, pages, image_ext)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            book_data.id,
            book_data.title_1,
            book_data.title_2,
            book_data.artists,
            book_data.groups,
            book_data.tags,
            book_data.languages,
            book_data.pages,
            ext,
        ],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

// =================Config/Init/Args Stuff=================
fn read_config(file_path: &Path) -> Result<Config, String> {
    let file = File::open(file_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("[-] Config file not found at: {}", file_path.display())
        } else {
            format!("[-] Failed to open config file: {}", e)
        }
	})?;
    let reader = BufReader::new(file);
    let config = serde_json::from_reader(reader).map_err(|e| e.to_string())?;
    Ok(config)
}

fn init_client(outdir: &Path) -> Result<Client, String> {
    let config = read_config(&outdir.join("config.json"))?;
    let mut headers = HeaderMap::new();

    headers.insert(USER_AGENT, HeaderValue::from_str(&config.user_agent).map_err(|e| e.to_string())?);
    let cookie_header = config.cookies.join("; ");
    if !cookie_header.is_empty() {
        headers.insert(COOKIE, HeaderValue::from_str(&cookie_header).map_err(|e| e.to_string())?);
    }

    let client = Client::builder()
        .emulation(Emulation::Firefox143)
        .default_headers(headers)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(client)
}

// We take input as 70-1 or 1-70, but internally we store them as 1-70 and just use desc order when iterating
fn parse_page_range(value: &str) -> Result<RangeInclusive<u32>, String> {
    let parts: Vec<&str> = value.split('-').collect();
    let first: u32 = parts.get(0)
        .and_then(|s| s.trim().parse().ok())
        .ok_or("Range format error")?;
    let second: u32 = parts.get(1)
        .and_then(|s| s.trim().parse().ok())
        .ok_or("Range format error")?;

    if first < 1 || second < 1 {
        return Err("Range format error".to_string());
    }

    let (start, end) = if first >= second {
        (second, first)
    } else {
        (first, second)
    };

    Ok(start..=end)
}

fn restore_database(outpath: &Path) -> Result<(), String> {
    let db_path = outpath.join(SQ_DB_FILE);
    let prevstate_dir = outpath.join(".prevstate");
    let prev_db_path = prevstate_dir.join(SQ_DB_FILE);

    if !prev_db_path.exists() {
        println!("[-] No previous database found at {}", prev_db_path.display());
        return Ok(());
    }

    let prev_count = get_total_books_in_db(&prev_db_path)?;
    println!("[*] Previous database contains {} books", prev_count);

    if db_path.exists() {
        let current_count = get_total_books_in_db(&db_path)?;
        println!("[*] Current database contains {} books", current_count);
        println!("[!] WARNING: Restoring will replace the current database");
    } else {
        println!("[*] No current database exists");
    }

    print!("\n[?] Do you want to restore from the previous database? (y/N): ");
    std::io::stdout().flush().unwrap();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).map_err(|e| e.to_string())?;

    if input.trim().to_lowercase() != "y" {
        println!("[*] Restore cancelled");
        return Ok(());
    }

    std::fs::copy(&prev_db_path, &db_path).map_err(|e| format!("Failed to restore database: {}", e))?;
    println!("[+] Database restored successfully from {}", prev_db_path.display());
    Ok(())
}

#[derive(FromArgs)]
/// Command-line tool for exporting nhentai favorite list
struct NhentaiFavsArgs {
    /// whether to download thumbnail image (default: false)
    #[argh(switch, short = 't')]
    thumbnail: bool,

    /// page range to fetch data from (e.g. 1-20, 20-1). data's are fetch from higher to lower page.
    #[argh(option, short = 'p', from_str_fn(parse_page_range))]
    page_range: Option<RangeInclusive<u32>>,

    /// convert existing database to csv format in outpath. no network operation performed.
    #[argh(switch, short = 'c')]
    cvt_csv: bool,

    /// restore previous state of database if currupted by previous run (dangerous).
    #[argh(switch)]
    restore: bool,

    /// output directory, may contain existing database
    #[argh(option, short = 'o')]
    outpath: String,
}

//===============Async Mains================
async fn download_image(client: &Client, url: &str, id: &str, out_dir: &Path, ext: &str) -> Result<(), String> {
    let file_path = out_dir.join(".thumbnails").join(format!("{}.{}", id, ext));

    if Path::new(&file_path).exists() {
        return Ok(());
    }

    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    std::fs::write(&file_path, bytes).map_err(|e| e.to_string())?;

    Ok(())
}

async fn process_all_books(
    books: &Vec<HentaiBook>,
    client: &wreq::Client,
    outpath: &Path,
    db_path: &Path,
    download_cover: bool,
    page_num: u32,
    dest_page: u32,
    stats: Stats,
) -> Result<(), String> {
    let total_books = books.len() as u32;
    let book_counter = Arc::new(AtomicU32::new(0));
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let pb = ProgressBar::new(total_books as u64);
    pb.set_style(
        ProgressStyle::with_template("[{wide_bar:.cyan/blue}] [{msg}] [{pos}/{len}]")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.set_message(format!("{}->{}", page_num, dest_page));

    for chunk in books.iter().rev().collect::<Vec<_>>().chunks(2) {
        let mut handles = Vec::new();
        let mut to_insert = Vec::new();

        for book in chunk {
            let id = extract_id_from_url(&book.url).unwrap_or_default();

            // Early skip if exists
            if let Ok(true) = book_exists_in_db(&conn, &id) {
                stats.inc_skipped();
                let current = book_counter.fetch_add(1, Ordering::SeqCst);
                pb.set_position(current as u64);
                continue;
            }

            let client = client.clone();
            let url = book.url.clone();
            let thumb_url = book.thumbnail_url.clone();
            let outpath = PathBuf::from(outpath);
            let counter = Arc::clone(&book_counter);

            let handle = task::spawn(async move {
                let result = process_single_book(&client, &url, &thumb_url, &outpath, download_cover).await;
                counter.fetch_add(1, Ordering::SeqCst);
                result
            });

            handles.push(handle);
        }

        for handle in handles {
            match handle.await {
                Ok(Ok((book_data, ext))) => to_insert.push((book_data, ext)),
                Ok(Err(e)) => {
                    eprintln!("[-] Book task failed: {}", e);
                }
                Err(e) => {
                    eprintln!("[-] Join error: {}", e);
                }
            }
        }

        for (book_data, ext) in to_insert.into_iter() {
            save_book_to_db(&conn, &book_data, &ext)?;
            stats.inc_added();
        }
        pb.set_position(book_counter.load(Ordering::SeqCst) as u64);
    }
    pb.finish();
    Ok(())
}

/// Process one book: fetch HTML + optionally image concurrently
async fn process_single_book(
    client: &wreq::Client,
    url: &str,
    thumbnail_url: &str,
    outpath: &Path,
    download_cover: bool,
) -> Result<(BookData, String), String> {
    let id = extract_id_from_url(url).unwrap_or_default();

    let html_fut = async {
        let html = client.get(url).send().await.map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
        Ok::<_, String>(html)
    };

    let ext = extract_image_extension(thumbnail_url);

    let image_fut = async {
        if download_cover {
            download_image(client, thumbnail_url, &id, outpath, &ext).await?;
        }
        Ok::<_, String>(())
    };

    let (html_res, _image_res) = join!(html_fut, image_fut);

    let json_buf = html_res?;
    let book_data = parse_book_json(&json_buf)?;
    Ok((book_data, ext))
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: NhentaiFavsArgs = argh::from_env();
    let outpath = PathBuf::from(&args.outpath);
    let db_path = outpath.join(SQ_DB_FILE);

    if args.cvt_csv {
        if db_path.exists() {
            export_db_to_csv(&db_path, &outpath.join("nfavs_export.csv"))?;
        } else {
            println!("[-] Database not found at {}", db_path.display());
        }
        return Ok(());
    }

    if args.restore {
        return restore_database(&outpath);
    }

    // Backup current database to .prevstate directory
    if db_path.exists() {
        let prevstate_dir = outpath.join(".prevstate");

        if !prevstate_dir.exists() {
            std::fs::create_dir_all(&prevstate_dir).map_err(|e| format!("Failed to create .prevstate directory: {}", e))?;
        }

        let prev_db_path = prevstate_dir.join(SQ_DB_FILE);
        std::fs::copy(&db_path, &prev_db_path).map_err(|e| format!("Failed to backup database: {}", e))?;
        println!("[+] Database backed up to {}", prev_db_path.display());
    }

    std::fs::create_dir_all(&outpath.join(".thumbnails")).map_err(|e| e.to_string())?;
    let db_path = outpath.join(SQ_DB_FILE);
    init_db(&db_path)?;
    let client = init_client(&outpath)?;
    let start = Instant::now();
    let stats = Stats::new();

    let resp = client.get(FAV_URL).send().await.map_err(|e| e.to_string())?;
    if resp.status() != 200 {
        return Err(format!(
            "[-] Failed to access nhentai.net: HTTP status code {}. Please check config.json for user agent and cookies.",
            resp.status()
        ));
    }

    let favpage = resp.text().await.map_err(|e| e.to_string())?;
    let login_data = parse_login_data(&favpage)?;
    println!("[+] Logged in as: {}", login_data.user_name);
    println!("[+] Total favorites: {}", login_data.fav_count);
    println!("[+] Total pages: {}", login_data.page_count);

    if login_data.page_count < 1 {
        return Err("[-] No favorite pages available".to_string());
    }
    if let Some(range) = &args.page_range {
        if *range.end() > login_data.page_count {
            return Err(format!("[-] End page {} exceeds total pages {}", range.end(), login_data.page_count));
        }
    }
    let page_range = args.page_range.clone().unwrap_or(1..=login_data.page_count);
	println!("[+] Fetching data from page {} to {}", page_range.end(), page_range.start());
	if args.thumbnail {
		println!("[+] Thumbnail download toggle ON");
	}
    println!();

    let page_start = *page_range.start();

    for page_num in page_range.rev() {
        let page_url = format!("{}?page={}", FAV_URL, page_num);
        let books = if page_num == 1 {
            parse_fav_page(&favpage)?
        } else {
            let resp = client.get(&page_url).send().await.map_err(|e| e.to_string())?;
            parse_fav_page(&resp.text().await.map_err(|e| e.to_string())?)?
        };
        process_all_books(&books, &client, &outpath, &db_path, args.thumbnail, page_num, page_start, stats.clone()).await?;
    }

    let total_in_db = get_total_books_in_db(&db_path)?;

    println!(); println!();
    println!("[+] Completed in {:.2?}", start.elapsed());
    println!("[+] Total skipped (already exists): {}", stats.get_skipped());
    println!("[+] Total new added: {}", stats.get_added());
    println!("[+] Total in database: {}", total_in_db);
    Ok(())
}
