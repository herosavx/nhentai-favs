mod api;
mod db;
mod models;

use anyhow::{Context, Result};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::{fs, path::{Path, PathBuf}};
use argh::FromArgs;

use api::NhenClient;
use db::{Database, TagsDatabase};
use models::Config;

#[derive(FromArgs, Debug)]
/// Local backup tool for nhentai favorites via v2 API.
struct Args {
    /// output directory for the database and config
    #[argh(option, short = 'o')]
    outpath: PathBuf,

    /// [Action] generate the tag bank from scratch safely
    #[argh(switch, short = 'g')]
    generate_tagbank: bool,

    /// [Action] convert existing database to csv format
    #[argh(switch, short = 'c')]
    cvt_csv: bool,

    /// [Action] restore nfavs.db from the previous state
    #[argh(switch, short = 'r')]
    restore: bool,

    /// [Action] download thumbnails for existing items in the database
    #[argh(switch, short = 't')]
    thumbnail: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Args = argh::from_env();
    fs::create_dir_all(&args.outpath)?;
    fs::create_dir_all(&args.outpath.join(".prevstate"))?;

    let mut db = Database::new(&args.outpath)?;

    if args.cvt_csv {
        return db.export_to_csv();
    }

    if args.restore {
        return db.restore_nfavs();
    }

    if args.thumbnail {
        return download_thumbnails(&db, &args.outpath).await;
    }

    if args.generate_tagbank {
        return generate_tags(&args.outpath).await;
    }

    // For Syncing, we need the config file for the API key
    let config_path = args.outpath.join("config.json");
    let config_str = fs::read_to_string(&config_path)
        .context("Could not read config.json. Ensure it contains your 'api_key'.")?;
    let config: Config = serde_json::from_str(&config_str)?;

    // Sync action
    let client = NhenClient::new(&config.api_key)?;
    db.backup_nfavs()?;
    sync_favorites(&client, &mut db).await?;

    Ok(())
}

async fn generate_tags(outpath: &Path) -> Result<()> {
    let target_types = ["artist", "group", "language", "tag"];
    let tmp_tags_path = outpath.join("tags.db.tmp");
    let real_tags_path = outpath.join("tags.db");
    let prev_tags_path = outpath.join(".prevstate").join("tags.db");

    // Start fresh for tmp
    if tmp_tags_path.exists() {
        fs::remove_file(&tmp_tags_path)?;
    }

    let tmp_db = TagsDatabase::new(&tmp_tags_path)?;
    let clean_client = NhenClient::clean_client()?;

    for t_type in target_types {
        println!("\n[*] Fetching tag type: {}", t_type);
        
        let initial_resp = NhenClient::get_tags_page(&clean_client, t_type, 1).await?;
        let pb = ProgressBar::new(initial_resp.num_pages as u64);

        for page in 1..=initial_resp.num_pages {
            let resp = NhenClient::get_tags_page(&clean_client, t_type, page).await?;
            for tag in resp.result {
                tmp_db.insert_tag(&tag)?;
            }
            pb.inc(1);
        }
        pb.finish_with_message("Done");
    }

    // Successfully grabbed everything. Now safely swap.
    if real_tags_path.exists() {
        fs::rename(&real_tags_path, &prev_tags_path)?;
        println!("\n[*] Old tags.db moved to .prevstate/tags.db");
    }
    fs::rename(&tmp_tags_path, &real_tags_path)?;
    
    println!("[+] Tag bank generation successfully complete.");
    Ok(())
}

async fn sync_favorites(client: &NhenClient, db: &mut Database) -> Result<()> {
    println!("[*] Fetching favorites metadata...");
    let initial_resp = client.get_favorites_page(1).await?;
    let total_pages = initial_resp.num_pages;

    println!("[*] Found {} pages. Starting Delta Sync...", total_pages);

    let pb = ProgressBar::new(total_pages as u64);
    pb.set_style(ProgressStyle::with_template("[{bar:40.cyan/blue}] Page {pos}/{len} ({msg})").unwrap());

    let mut new_items_buffer = Vec::new();
    let mut stopped_early = false;

    // Scan forward: Page 1 is the newest data. We want to stop as soon as we hit the old data.
    for page in 1..=total_pages {
        pb.set_message("Scanning...");
        let resp = client.get_favorites_page(page).await?;
        
        let items = resp.result;
        if items.is_empty() {
            break;
        }

        let items_on_page = items.len();
        let mut known_local_ids = Vec::new();

        for item in items {
            if let Some(local_id) = db.get_local_id(item.id)? {
                known_local_ids.push(local_id);
            } else {
                new_items_buffer.push(item);
            }
        }

        // --- CRITICAL LOGIC: OVERLAP DETECTION ---
        // We require 72% of the page to be "known" items before we even bother checking sequence.
        // If a page has 25 items, threshold is 18. If it's the last page and has 6 items, threshold is 5.
        let threshold = (items_on_page as f32 * 0.72).ceil() as usize;

        if known_local_ids.len() >= threshold {

            // --- CRITICAL LOGIC: THE SEQUENCE VALIDATOR ---
            // Protects against "Mass Bumps" (User unfavoriting and refavoriting old items).
            // Because we ALWAYS insert oldest->newest, an untouched block of old favorites
            // will always have STRICTLY DESCENDING local_ids when read from top to bottom of a webpage.

            // If the page has known items, the sequence is at least 1. Otherwise 0.
            let mut max_sequence = if known_local_ids.is_empty() { 0 } else { 1 };
            let mut current_sequence = 1;

            for window in known_local_ids.windows(2) {
                let current = window[0];
                let next = window[1];

                // Rule 1: 'current' must be greater than 'next' (Descending order).
                // Rule 2: The gap between them must be <= 3.
                // Why 3?
                // Gap of 1 (100 - 99) = Perfect sequence.
                // Gap of 2 (100 - 98) = 1 item was deleted from the website.
                // Gap of 3 (100 - 97) = 2 adjacent items were deleted from the website.
                if current > next && (current - next) <= 3 {
                    current_sequence += 1;
                    if current_sequence > max_sequence {
                        max_sequence = current_sequence;
                    }
                } else {
                    // Sequence broken (either out of order due to a bump, or a massive deletion gap)
                    current_sequence = 1;
                }
            }

            // If our unbroken descending sequence is long enough to meet the threshold,
            // we are mathematically certain we have hit the untouched "old data" block.
            if max_sequence >= threshold {
                pb.set_message(format!("Solid overlap hit (Chain of {} items).", max_sequence));
                stopped_early = true;
                pb.inc(1);
                break;
            } else {
                // If we get here, the user likely did a Mass Bump, OR the website deleted 3+ contiguous items.
                // We fail safely by continuing the scan to the next page.
                pb.suspend(|| {
                    println!("\n[*] Warning: Found {} known items on Page {}, but they were fragmented (Max chain: {}). Continuing scan...", known_local_ids.len(), page, max_sequence);
                });
            }
        }
        pb.inc(1);
    }

    if stopped_early {
        pb.finish_with_message("Delta sync caught up!");
    } else {
        pb.finish_with_message("Full scan complete.");
    }

    if new_items_buffer.is_empty() {
        println!("\n[+] Sync Complete! No new favorites found.");
        return Ok(());
    }

    println!("\n[*] Reversing chronological order and inserting {} new items...", new_items_buffer.len());

    // --- CHRONOLOGICAL REVERSAL ---
    // We scraped from Page 1 (Newest) to Page N (Oldest).
    // To keep our local_id auto-incrementing in chronological order (Oldest -> Newest),
    // we MUST reverse the buffer before inserting.
    new_items_buffer.reverse();

    db.insert_favs_batch(&new_items_buffer)?;

    println!("[+] Sync Complete! Successfully added {} new favorites.", new_items_buffer.len());
    Ok(())
}

async fn download_thumbnails(db: &Database, outpath: &Path) -> Result<()> {
    let thumb_dir = outpath.join(".thumbnails");
    fs::create_dir_all(&thumb_dir)?;

    let items = db.get_all_thumbnails()?;
    println!("[*] Found {} records to check for thumbnails.", items.len());

    let pb = ProgressBar::new(items.len() as u64);
    pb.set_style(ProgressStyle::with_template("[{bar:40.cyan/blue}] {pos}/{len} ({msg})").unwrap());

    let client = NhenClient::clean_client()?;
    let mirrors = ["t1", "t2", "t3", "t4", "t5"];

    // Buffer unordered allows 4 concurrent downloads at a time.
    futures::stream::iter(items)
        .map(|(nhen_id, thumb_path)| {
            let thumb_dir = thumb_dir.clone();
            let client = client.clone();
            let pb = pb.clone();
            
            async move {
                let ext = thumb_path.split('.').last().unwrap_or("jpg");
                let dest_path = thumb_dir.join(format!("{}.{}", nhen_id, ext));

                if !dest_path.exists() {
                    let mut success = false;
                    for mirror in mirrors {
                        let url = format!("https://{}.nhentai.net/{}", mirror, thumb_path);
                        if let Ok(resp) = client.get(&url).send().await {
                            if resp.status().is_success() {
                                if let Ok(bytes) = resp.bytes().await {
                                    let _ = fs::write(&dest_path, bytes);
                                    success = true;
                                    break;
                                }
                            }
                        }
                    }
                    if !success {
                        pb.set_message(format!("Failed: {}", nhen_id));
                    }
                }
                pb.inc(1);
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<()>>()
        .await;

    pb.finish_with_message("Finished downloading thumbnails.");
    Ok(())
}
