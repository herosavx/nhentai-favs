use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct FavResponse {
    pub result: Vec<FavItem>,
    pub num_pages: u32,
}

#[derive(Deserialize, Debug)]
pub struct FavItem {
    pub id: u32,
    pub thumbnail: String,
    pub english_title: Option<String>,
    pub japanese_title: Option<String>,
    pub num_pages: u32,
    pub tag_ids: Vec<u32>,
}

#[derive(Deserialize, Debug)]
pub struct TagResponse {
    pub result: Vec<TagItem>,
    pub num_pages: u32,
}

#[derive(Deserialize, Debug)]
pub struct TagItem {
    pub id: u32,
    #[serde(rename = "type")]
    pub tag_type: String,
    pub name: String,
    pub count: u32,
}

#[derive(Deserialize, Debug)]
pub struct ApiError {
    pub error: String,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub api_key: String,
}
