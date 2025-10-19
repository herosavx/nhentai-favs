mod cli;
mod config;
mod database;
mod error;
mod models;
mod nhen_client;
mod parser;
mod scraper;
mod stats;

use std::path::PathBuf;
use std::time::Instant;

use crate::{
    cli::Args, database::*, error::Result, nhen_client::NHentaiClient, scraper::FavoritesScraper,
    stats::Stats,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let outpath = PathBuf::from(&args.outpath);
    let db_path = outpath.join(DB_FILENAME);
    let prevstate_dir = outpath.join(".prevstate");

    if args.export_csv {
        return handle_csv_export(&db_path, &outpath);
    }

    if args.restore {
        return restore_database(&db_path, &prevstate_dir);
    }

    run_scraper(args, outpath, db_path, prevstate_dir).await
}

fn handle_csv_export(db_path: &PathBuf, outpath: &PathBuf) -> Result<()> {
    if db_path.exists() {
        let db = Database::new(db_path)?;
        db.export_to_csv(&outpath.join("nfavs_export.csv"))?;
    } else {
        println!("[-] Database not found at {}", db_path.display());
    }
    Ok(())
}

async fn run_scraper(
    args: Args,
    outpath: PathBuf,
    db_path: PathBuf,
    prevstate_dir: PathBuf,
) -> Result<()> {
    // Backup existing database
    backup_database(&db_path, &prevstate_dir)?;

    std::fs::create_dir_all(&outpath.join(".thumbnails"))?;

    let database = Database::new(&db_path)?;
    database.initialize()?;

    let client = NHentaiClient::new(&outpath.join("config.json"))?;
    let scraper = FavoritesScraper::new(client, database, outpath.clone());

    let start = Instant::now();
    let stats = Stats::new();

    // Fetch user data and validate
    let (user_data, first_page_html) = scraper.fetch_user_data().await?;

    print_user_info(&user_data);

    if user_data.is_empty() {
        println!("[-] No favorite pages available");
        return Ok(());
    }

    args.validate_page_range(user_data.page_count)?;
    let page_range = args.get_page_range(user_data.page_count);

    print_scraping_info(&page_range, args.thumbnail);

    // Scrape pages
    let page_start = *page_range.start();
    for page_num in page_range.rev() {
        let cached_page = if page_num == 1 {
            Some(first_page_html.as_str())
        } else {
            None
        };

        let books = scraper.scrape_page(page_num, cached_page).await?;

        scraper
            .process_all_books(&books, args.thumbnail, page_num, page_start, stats.clone())
            .await?;
    }

    // Print summary
    let db = Database::new(&db_path)?;
    let total_in_db = db.count_books()?;

    println!("[+] Completed in {:.2?}", start.elapsed());
    stats.print_summary(total_in_db);

    Ok(())
}

fn print_user_info(user_data: &crate::models::UserData) {
    println!("[+] Logged in as: {}", user_data.user_name);
    println!("[+] Total favorites: {}", user_data.fav_count);
    println!("[+] Total pages: {}", user_data.page_count);
}

fn print_scraping_info(page_range: &std::ops::RangeInclusive<u32>, thumbnail: bool) {
    println!(
        "[+] Fetching data from page {} to {}",
        page_range.end(),
        page_range.start()
    );
    if thumbnail {
        println!("[+] Thumbnail download toggle ON");
    }
    println!();
}
