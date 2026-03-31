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

    let db = Database::new(&args.outpath)?;

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
    sync_favorites(&client, &db).await?;

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

async fn sync_favorites(client: &NhenClient, db: &Database) -> Result<()> {
    println!("[*] Fetching favorites metadata...");
    let initial_resp = client.get_favorites_page(1).await?;
    let total_pages = initial_resp.num_pages;

    println!("[*] Found {} pages. Syncing from oldest to newest...", total_pages);
    let pb = ProgressBar::new(total_pages as u64);

    let mut added = 0;
    let mut skipped = 0;

    for page in (1..=total_pages).rev() {
        let resp = client.get_favorites_page(page).await?;
        
        let mut items = resp.result;
        items.reverse();

        for item in items {
            if db.fav_exists(item.id)? {
                skipped += 1;
                continue;
            }
            db.insert_fav(&item)?;
            added += 1;
        }
        pb.inc(1);
    }

    pb.finish();
    println!("\n[+] Sync Complete! Added: {}, Skipped: {}", added, skipped);
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
