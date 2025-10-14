use wreq::Client;
use wreq::header::{HeaderMap, HeaderValue, USER_AGENT, COOKIE};
use wreq_util::Emulation;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use regex::Regex;
use std::time::Instant;
use argh::FromArgs;
use std::ops::RangeInclusive;
use rusqlite::{Connection, params};
use tokio::{join, task};
use std::path::{Path, PathBuf};

const FAV_BASE_URL: &str = "https://nhentai.net/favorites/";
const BASE_URL: &str = "https://nhentai.net";
const SQ_DB_FILE: &str = "nfavs.db";

#[derive(Serialize, Deserialize)]
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

#[derive(Debug)]
struct UserData {
    user_name: String,
    page_count: u32,
    fav_count: u32,
}

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
            .map(|href| format!("{}{}", BASE_URL, href))
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

fn parse_book_page(html: &str, url: &str) -> Result<BookData, String> {
    let document = Html::parse_document(html);

    let h1_sel = Selector::parse("h1.title").map_err(|e| e.to_string())?;
    let h2_sel = Selector::parse("h2.title").map_err(|e| e.to_string())?;
    let tag_container_sel = Selector::parse("div.tag-container").map_err(|e| e.to_string())?;
    let name_sel = Selector::parse("span.name").map_err(|e| e.to_string())?;

    let title_1 = document
        .select(&h1_sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    let title_2 = document
        .select(&h2_sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    let mut artists = String::new();
    let mut groups = String::new();
    let mut tags = String::new();
    let mut languages = String::new();
    let mut pages: u32 = 0;

    for div in document.select(&tag_container_sel) {
        let class = div.value().attr("class").unwrap_or("");
        if class.contains("hidden") {
            continue;
        }

        let label = div.text().take(1).collect::<String>();
        let lower = label.to_lowercase();

        let names: Vec<String> = div
            .select(&name_sel)
            .map(|n| n.text().collect::<String>().trim().to_string())
            .collect();

        if lower.contains("artists:") {
            artists = names.join(", ");
        } else if lower.contains("groups:") {
            groups = names.join(", ");
        } else if lower.contains("tags:") {
            tags = names.join(", ");
        } else if lower.contains("languages:") {
            languages = names.join(", ");
        } else if lower.contains("pages:") {
            pages = names
                .get(0)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
        }
    }

    let id = extract_id_from_url(url).unwrap_or_default();

    Ok(BookData {
        title_1,
        title_2,
        artists,
        groups,
        tags,
        languages,
        id,
        pages,
    })
}

fn extract_id_from_url(url: &str) -> Option<String> {
    let re = Regex::new(r"/g/(\d+)/?").unwrap();
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
            id TEXT PRIMARY KEY,
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

fn export_db_to_csv(db_path: &Path, out_path: &Path) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT * FROM books ORDER BY id").map_err(|e| e.to_string())?;
    
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
        "id",
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
    let mut stmt = conn.prepare("SELECT id FROM books WHERE id = ?1").map_err(|e| e.to_string())?;
    let exists = stmt.exists(params![id]).map_err(|e| e.to_string())?;
    Ok(exists)
}

fn save_book_to_db(conn: &Connection, book_data: &BookData, ext: &str) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO books (id, title_1, title_2, artists, groups, tags, languages, pages, image_ext)
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
fn read_config(file_path: &str) -> Result<Config, String> {
    let file = File::open(file_path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let config = serde_json::from_reader(reader).map_err(|e| e.to_string())?;
    Ok(config)
}

fn init_client() -> Result<Client, String> {
    let config = read_config("config.json")?;
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

fn parse_page_range(value: &str) -> Result<RangeInclusive<u32>, String> {
    let parts: Vec<&str> = value.split('-').collect();
    let start: u32 = parts.get(0)
        .and_then(|s| s.trim().parse().ok())
        .ok_or("Range format error")?;
    let end: u32 = parts.get(1)
        .and_then(|s| s.trim().parse().ok())
        .ok_or("Range format error")?;

    if start < 1 || start > end {
        return Err("Range format error".to_string());
    }

    Ok(start..=end)
}

#[derive(FromArgs)]
/// Command-line tool for exporting nhentai favorite list
struct NhentaiFavsArgs {
    /// whether to download thumbnail image (default: false)
    #[argh(switch, short = 't')]
    thumbnail: bool,

    /// page range to fetch data from (e.g., 1-20, 1-1)
    #[argh(option, short = 'p', from_str_fn(parse_page_range))]
    page_range: Option<RangeInclusive<u32>>,

    /// convert existing database to csv format in outpath. no network operation performed.
    #[argh(switch, short = 'c')]
    cvt_csv: bool,

    /// output directory, may contain existing database
    #[argh(option, short = 'o')]
    outpath: String,
}

//===============Async Mains================
async fn download_image(client: &Client, url: &str, id: &str, out_dir: &Path, ext: &str) -> Result<(), String> {
    let file_path = out_dir.join("thumbnails").join(format!("{}.{}", id, ext));

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
) -> Result<(), String> {
    for chunk in books.chunks(2) {
        let mut handles = Vec::new();

        for book in chunk {
            let client = client.clone();
            let url = book.url.clone();
            let thumb_url = book.thumbnail_url.clone();
            let outpath = PathBuf::from(outpath);
            let db_path = PathBuf::from(db_path);

            let handle = task::spawn(async move {
                if let Err(e) = process_single_book(&client, &url, &thumb_url, &outpath, &db_path, download_cover).await {
                    eprintln!("[-] Book task error ({}): {}", url, e);
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }
    }

    Ok(())
}

/// Process one book: fetch HTML + optionally image concurrently
async fn process_single_book(
    client: &wreq::Client,
    url: &str,
    thumbnail_url: &str,
    outpath: &Path,
    db_path: &Path,
    download_cover: bool,
) -> Result<(), String> {
    let id = extract_id_from_url(url).unwrap_or_default();

    if let Ok(conn) = Connection::open(db_path) {
        if let Ok(true) = book_exists_in_db(&conn, &id) {
            println!("[~] Already in DB: {}", id);
            return Ok(());
        }
    }

    println!("[+] Fetching {}", url);

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

    let html_buf = html_res?;
    let book_data = parse_book_page(&html_buf, url)?;

    if let Ok(conn) = Connection::open(db_path) {
        save_book_to_db(&conn, &book_data, &ext)?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: NhentaiFavsArgs = argh::from_env();

    if args.cvt_csv {
        let db_path = PathBuf::from(&args.outpath).join(SQ_DB_FILE);
        if Path::new(&db_path).exists() {
            export_db_to_csv(&db_path, &PathBuf::from(&args.outpath).join("nfavs_export.csv"))?;
        } else {
            println!("[-] Database not found at {}", db_path.display());
        }
        return Ok(());
    }

    std::fs::create_dir_all(&PathBuf::from(&args.outpath).join("thumbnails")).map_err(|e| e.to_string())?;
    let db_path = PathBuf::from(&args.outpath).join(SQ_DB_FILE);
    init_db(&db_path)?;
    let client = init_client()?;
    let start = Instant::now();

    let resp = client.get(FAV_BASE_URL).send().await.map_err(|e| e.to_string())?;
    if resp.status() != 200 {
        return Err("[-] Unable to access nhentai, possibly CF?".to_string());
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

    for page_num in page_range {
        let page_url = format!("{}?page={}", FAV_BASE_URL, page_num);
        println!("\n[*] Fetching page {}", page_num);
        let books = if page_num == 1 {
            parse_fav_page(&favpage)?
        } else {
            let resp = client.get(&page_url).send().await.map_err(|e| e.to_string())?;
            parse_fav_page(&resp.text().await.map_err(|e| e.to_string())?)?
        };
        process_all_books(&books, &client, &PathBuf::from(&args.outpath), &db_path, args.thumbnail).await?;
    }
    println!("[+] Completed in {:.2?}", start.elapsed());
    Ok(())
}
