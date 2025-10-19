use crate::error::{AppError, Result};
use crate::models::{BookData, NhentaiResponse, NhentaiTag};

pub fn parse_book_json(json_str: &str) -> Result<BookData> {
    let parsed: NhentaiResponse = serde_json::from_str(json_str)?;

    match parsed {
        NhentaiResponse::Error { error } => Err(AppError::Gallery(error)),
        NhentaiResponse::Gallery {
            id,
            title,
            tags,
            num_pages,
        } => {
            let (artists, groups, tags_vec, languages) = categorize_tags(tags);

            Ok(BookData::new(
                id.to_string(),
                title.english.unwrap_or_default(),
                title.japanese.unwrap_or_default(),
                NhentaiTag::sort_by_popularity(artists).join(", "),
                NhentaiTag::sort_by_popularity(groups).join(", "),
                NhentaiTag::sort_by_popularity(tags_vec).join(", "),
                NhentaiTag::sort_by_popularity(languages).join(", "),
                num_pages,
            ))
        }
    }
}

fn categorize_tags(
    tags: Vec<NhentaiTag>,
) -> (
    Vec<NhentaiTag>,
    Vec<NhentaiTag>,
    Vec<NhentaiTag>,
    Vec<NhentaiTag>,
) {
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

    (artists, groups, tags_vec, languages)
}
