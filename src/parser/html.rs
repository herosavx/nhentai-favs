use crate::error::{AppError, Result};
use crate::models::book::extract_id_from_url;
use crate::models::{Book, UserData};
use crate::nhen_client::GALLERY_URL;
use scraper::{Html, Selector};

pub fn parse_login_data(html: &str) -> Result<UserData> {
    let document = Html::parse_document(html);

    let username_selector =
        Selector::parse("span.username").map_err(|e| AppError::ParseError(e.to_string()))?;
    let count_selector =
        Selector::parse("span.count").map_err(|e| AppError::ParseError(e.to_string()))?;
    let lastpage_selector =
        Selector::parse("a.last").map_err(|e| AppError::ParseError(e.to_string()))?;

    let user_name = document
        .select(&username_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| AppError::LoginFailed("Username not found".to_string()))?;

    let fav_count = document
        .select(&count_selector)
        .next()
        .and_then(|el| {
            let txt = el.text().collect::<String>();
            let cleaned = txt.replace(['(', ')', ','], "").trim().to_string();
            cleaned.parse::<u32>().ok()
        })
        .unwrap_or(0);

    let page_count = document
        .select(&lastpage_selector)
        .next()
        .and_then(|el| el.value().attr("href"))
        .and_then(|href| href.split("page=").nth(1)?.parse::<u32>().ok())
        .unwrap_or_else(|| if fav_count > 0 { 1 } else { 0 });

    Ok(UserData::new(user_name, page_count, fav_count))
}

pub fn parse_fav_page(html: &str) -> Result<Vec<Book>> {
    let document = Html::parse_document(html);

    let gallery_selector =
        Selector::parse("div.gallery-favorite").map_err(|e| AppError::ParseError(e.to_string()))?;
    let img_selector =
        Selector::parse("img.lazyload").map_err(|e| AppError::ParseError(e.to_string()))?;
    let link_selector =
        Selector::parse("a.cover").map_err(|e| AppError::ParseError(e.to_string()))?;

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
            .and_then(extract_id_from_url)
            .map(|id| format!("{}{}", GALLERY_URL, id))
            .unwrap_or_default();

        match Book::new(url.clone(), thumbnail_url.clone()) {
            Some(book) => books.push(book),
            None => {
                eprintln!(
                    "[-] Skipping gallery due to missing fields: url='{}', thumbnail='{}'",
                    url, thumbnail_url
                );
            }
        }
    }

    Ok(books)
}
