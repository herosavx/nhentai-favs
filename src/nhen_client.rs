use crate::config::Config;
use crate::error::Result;
use std::path::Path;
use wreq::Client;
use wreq::header::{COOKIE, HeaderMap, HeaderValue, USER_AGENT};
use wreq_util::Emulation;

pub const FAV_URL: &str = "https://nhentai.net/favorites/";
pub const GALLERY_URL: &str = "https://nhentai.net/api/gallery/";

pub struct NHentaiClient {
    client: Client,
}

impl NHentaiClient {
    pub fn new(config_path: &Path) -> Result<Self> {
        let config = Config::from_file(config_path)?;
        let client = Self::build_client(&config)?;
        Ok(Self { client })
    }

    fn build_client(config: &Config) -> Result<Client> {
        let mut headers = HeaderMap::new();

        headers.insert(USER_AGENT, HeaderValue::from_str(&config.user_agent)?);

        let cookie_header = config.cookie_header();
        if !cookie_header.is_empty() {
            headers.insert(COOKIE, HeaderValue::from_str(&cookie_header)?);
        }

        let client = Client::builder()
            .emulation(Emulation::Firefox143)
            .default_headers(headers)
            .build()?;

        Ok(client)
    }

    pub fn inner(&self) -> &Client {
        &self.client
    }

    pub async fn get(&self, url: &str) -> Result<wreq::Response> {
        Ok(self.client.get(url).send().await?)
    }

    pub async fn get_text(&self, url: &str) -> Result<String> {
        let response = self.get(url).await?;
        Ok(response.text().await?)
    }
}
