use regex::Regex;

#[derive(Debug, Clone)]
pub struct Book {
    pub url: String,
    pub thumbnail_url: String,
}

impl Book {
    pub fn new(url: String, thumbnail_url: String) -> Option<Self> {
        if url.is_empty() || thumbnail_url.is_empty() {
            None
        } else {
            Some(Self { url, thumbnail_url })
        }
    }

    pub fn id(&self) -> Option<String> {
        extract_id_from_url(&self.url)
    }

    pub fn image_extension(&self) -> String {
        self.thumbnail_url
            .split('.')
            .last()
            .and_then(|ext| ext.split('?').next())
            .unwrap_or("jpg")
            .to_string()
    }
}

#[derive(Debug, Clone)]
pub struct BookData {
    pub id: String,
    pub title_1: String,
    pub title_2: String,
    pub artists: String,
    pub groups: String,
    pub tags: String,
    pub languages: String,
    pub pages: u32,
}

impl BookData {
    pub fn new(
        id: String,
        title_1: String,
        title_2: String,
        artists: String,
        groups: String,
        tags: String,
        languages: String,
        pages: u32,
    ) -> Self {
        Self {
            id,
            title_1,
            title_2,
            artists,
            groups,
            tags,
            languages,
            pages,
        }
    }
}

pub fn extract_id_from_url(url: &str) -> Option<String> {
    let re = Regex::new(r"(?:/g/|/gallery/)(\d+)/?").unwrap();
    re.captures(url)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
}
