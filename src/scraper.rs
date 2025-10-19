use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::{join, task};

use crate::database::Database;
use crate::error::{AppError, Result};
use crate::models::{Book, BookData, UserData};
use crate::nhen_client::{FAV_URL, NHentaiClient};
use crate::parser::{parse_book_json, parse_fav_page, parse_login_data};
use crate::stats::Stats;

pub struct FavoritesScraper {
    client: NHentaiClient,
    database: Database,
    out_dir: PathBuf,
}

impl FavoritesScraper {
    pub fn new(client: NHentaiClient, database: Database, out_dir: PathBuf) -> Self {
        Self {
            client,
            database,
            out_dir,
        }
    }

    pub async fn fetch_user_data(&self) -> Result<(UserData, String)> {
        let response = self.client.get(FAV_URL).await?;

        if response.status() != 200 {
            return Err(AppError::LoginFailed(format!(
                "HTTP status code {}. Please check config.json for user agent and cookies.",
                response.status()
            )));
        }

        let html = response.text().await?;
        let user_data = parse_login_data(&html)?;

        Ok((user_data, html))
    }

    pub async fn scrape_page(
        &self,
        page_num: u32,
        cached_first_page: Option<&str>,
    ) -> Result<Vec<Book>> {
        let html = if page_num == 1 && cached_first_page.is_some() {
            cached_first_page.unwrap().to_string()
        } else {
            let url = format!("{}?page={}", FAV_URL, page_num);
            self.client.get_text(&url).await?
        };

        parse_fav_page(&html)
    }

    pub async fn process_all_books(
        &self,
        books: &[Book],
        download_cover: bool,
        page_num: u32,
        dest_page: u32,
        stats: Stats,
    ) -> Result<()> {
        let total_books = books.len() as u32;
        let book_counter = Arc::new(AtomicU32::new(0));

        let pb = create_progress_bar(total_books, page_num, dest_page);

        for chunk in books.iter().rev().collect::<Vec<_>>().chunks(2) {
            let mut handles = Vec::new();
            let mut to_insert = Vec::new();

            for book in chunk {
                let id = book.id().unwrap_or_default();

                // Early skip if exists
                if self.database.book_exists(&id)? {
                    stats.inc_skipped();
                    book_counter.fetch_add(1, Ordering::SeqCst);
                    pb.set_position(book_counter.load(Ordering::SeqCst) as u64);
                    continue;
                }

                // Clone the inner wreq::Client for the task
                let client_inner = self.client.inner().clone();
                let url = book.url.clone();
                let thumb_url = book.thumbnail_url.clone();
                let outpath = self.out_dir.clone();
                let counter = Arc::clone(&book_counter);

                let handle = task::spawn(async move {
                    let result = process_single_book_with_client(
                        &client_inner,
                        &url,
                        &thumb_url,
                        &outpath,
                        download_cover,
                    )
                    .await;
                    counter.fetch_add(1, Ordering::SeqCst);
                    result
                });

                handles.push(handle);
            }

            for handle in handles {
                match handle.await {
                    Ok(Ok((book_data, ext))) => to_insert.push((book_data, ext)),
                    Ok(Err(e)) => eprintln!("[-] Book task failed: {}", e),
                    Err(e) => eprintln!("[-] Join error: {}", e),
                }
            }

            for (book_data, ext) in to_insert {
                self.database.save_book(&book_data, &ext)?;
                stats.inc_added();
            }

            pb.set_position(book_counter.load(Ordering::SeqCst) as u64);
        }

        pb.finish();
        Ok(())
    }
}

/// Process a single book using the raw wreq::Client
async fn process_single_book_with_client(
    client: &wreq::Client,
    url: &str,
    thumbnail_url: &str,
    outpath: &Path,
    download_cover: bool,
) -> Result<(BookData, String)> {
    let book = Book::new(url.to_string(), thumbnail_url.to_string())
        .ok_or_else(|| AppError::ParseError("Invalid book data".to_string()))?;

    let id = book
        .id()
        .ok_or_else(|| AppError::ParseError("Cannot extract ID".to_string()))?;
    let ext = book.image_extension();

    let html_fut = async {
        let resp = client.get(url).send().await.map_err(AppError::Http)?;
        resp.text().await.map_err(AppError::Http)
    };

    let image_fut = async {
        if download_cover {
            let thumb_path = outpath.join(".thumbnails").join(format!("{}.{}", id, ext));

            if !thumb_path.exists() {
                let resp = client
                    .get(thumbnail_url)
                    .send()
                    .await
                    .map_err(AppError::Http)?;
                let bytes = resp.bytes().await.map_err(AppError::Http)?;
                std::fs::write(&thumb_path, bytes.to_vec())?;
            }
        }
        Ok::<_, AppError>(())
    };

    let (html_res, _image_res) = join!(html_fut, image_fut);
    let json_buf = html_res?;
    let book_data = parse_book_json(&json_buf)?;

    Ok((book_data, ext))
}

fn create_progress_bar(total: u32, page_num: u32, dest_page: u32) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::with_template("[{wide_bar:.cyan/blue}] [{msg}] [{pos}/{len}]")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.set_message(format!("{}->{}", page_num, dest_page));
    pb
}
