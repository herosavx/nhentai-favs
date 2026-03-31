use anyhow::{Context, Result};
use reqwest::{Client, Response, header};
use std::time::Duration;
use tokio::time::sleep;

use crate::models::{ApiError, FavResponse, TagResponse};

pub struct NhenClient {
    http: Client,
}

impl NhenClient {
    pub fn new(api_key: &str) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::ACCEPT, header::HeaderValue::from_static("application/json"));
        
        let auth_val = format!("Key {}", api_key);
        headers.insert(header::AUTHORIZATION, header::HeaderValue::from_str(&auth_val)?);

        let http = Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self { http })
    }

    pub fn clean_client() -> Result<Client> {
        Ok(Client::builder().build()?)
    }

    async fn handle_response<T: serde::de::DeserializeOwned>(resp: Response) -> Result<T> {
        if resp.status().is_success() {
            Ok(resp.json::<T>().await?)
        } else {
            let status = resp.status();
            if let Ok(api_err) = resp.json::<ApiError>().await {
                anyhow::bail!("API Error ({}): {}", status, api_err.error);
            } else {
                anyhow::bail!("HTTP Error: {}", status);
            }
        }
    }

    pub async fn get_favorites_page(&self, page: u32) -> Result<FavResponse> {
        // Limit: 30/min -> 1 every 2s. Targeting ~95% limit means 1 every 2.15s (2150ms).
        sleep(Duration::from_millis(2150)).await;
        
        let url = format!("https://nhentai.net/api/v2/favorites?page={}", page);
        let resp = self.http.get(&url).send().await?;
        
        Self::handle_response::<FavResponse>(resp).await
            .context(format!("Failed to fetch favorites page {}", page))
    }

    pub async fn get_tags_page(client: &Client, tag_type: &str, page: u32) -> Result<TagResponse> {
        // Limit: 60/min -> 1 every 1s. Targeting ~95% limit means 1 every 1.06s (1060ms). No auth.
        sleep(Duration::from_millis(1060)).await;
        
        let url = format!("https://nhentai.net/api/v2/tags/{}?sort=name&page={}&per_page=100", tag_type, page);
        let resp = client.get(&url).send().await?;
        
        Self::handle_response::<TagResponse>(resp).await
            .context(format!("Failed to fetch {} tags page {}", tag_type, page))
    }
}
