use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
pub struct NhentaiTitle {
    pub english: Option<String>,
    pub japanese: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NhentaiTag {
    #[serde(rename = "type")]
    pub tag_type: String,
    pub name: String,
    pub count: u32,
}

impl NhentaiTag {
    pub fn sort_by_popularity(mut tags: Vec<Self>) -> Vec<String> {
        tags.sort_by(|a, b| b.count.cmp(&a.count));
        tags.into_iter().map(|t| t.name).collect()
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum NhentaiResponse {
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
        StringOrNumber::String(s) => s
            .parse()
            .map_err(|_| serde::de::Error::custom(format!("Failed to parse '{}' as u32", s))),
        StringOrNumber::Number(n) => Ok(n),
    }
}
