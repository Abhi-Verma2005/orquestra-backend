use anyhow::Result;
use serde_json::Value;
use crate::services::cache::CacheService;

#[derive(Clone)]
pub struct MetadataService {
    cache: CacheService,
}

impl MetadataService {
    pub fn new(cache: CacheService) -> Self {
        Self { cache }
    }

    pub async fn get_solana_metadata(&self, mint_address: &str) -> Result<(Option<String>, Option<String>)> {
        if let Some(cached) = self.cache.get_metadata(mint_address, "solana").await? {
            let name = cached.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
            let logo_uri = cached.get("logoURI").and_then(|v| v.as_str()).map(|s| s.to_string());
            return Ok((name, logo_uri));
        }

        let (name, logo_uri) = self.fetch_jupiter_metadata(mint_address).await?;
        
        let metadata = serde_json::json!({
            "name": name.clone(),
            "logoURI": logo_uri.clone(),
        });
        
        let ttl_seconds = std::env::var("CACHE_TTL_SECONDS")
            .unwrap_or_else(|_| "3600".to_string())
            .parse()
            .unwrap_or(3600);
        self.cache.set_metadata(mint_address, "solana", &metadata, ttl_seconds).await?;

        Ok((name, logo_uri))
    }

    pub async fn get_ethereum_metadata(&self, token_address: &str) -> Result<(Option<String>, Option<String>)> {
        if let Some(cached) = self.cache.get_metadata(token_address, "ethereum").await? {
            let name = cached.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
            let logo_uri = cached.get("logoURI").and_then(|v| v.as_str()).map(|s| s.to_string());
            return Ok((name, logo_uri));
        }

        let (name, logo_uri) = self.fetch_coingecko_metadata(token_address).await?;
        
        let metadata = serde_json::json!({
            "name": name.clone(),
            "logoURI": logo_uri.clone(),
        });
        
        let ttl_seconds = std::env::var("CACHE_TTL_SECONDS")
            .unwrap_or_else(|_| "3600".to_string())
            .parse()
            .unwrap_or(3600);
        self.cache.set_metadata(token_address, "ethereum", &metadata, ttl_seconds).await?;

        Ok((name, logo_uri))
    }

    async fn fetch_jupiter_metadata(&self, mint_address: &str) -> Result<(Option<String>, Option<String>)> {
        let url = std::env::var("JUPITER_TOKEN_LIST_URL")
            .unwrap_or_else(|_| "https://token.jup.ag/strict".to_string());
        
        let response: Value = reqwest::get(&url).await?.json().await?;
        
        if let Some(tokens) = response.as_array() {
            for token in tokens {
                if let Some(addr) = token.get("address").and_then(|v| v.as_str()) {
                    if addr == mint_address {
                        let name = token.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                        let logo_uri = token.get("logoURI").and_then(|v| v.as_str()).map(|s| s.to_string());
                        return Ok((name, logo_uri));
                    }
                }
            }
        }

        Ok((None, None))
    }

    async fn fetch_coingecko_metadata(&self, token_address: &str) -> Result<(Option<String>, Option<String>)> {
        let coingecko_id = match token_address.to_lowercase().as_str() {
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48" => Some("usd-coin"),
            "0xdac17f958d2ee523a2206206994597c13d831ec7" => Some("tether"),
            "0x6b175474e89094c44da98b954eedeac495271d0f" => Some("dai"),
            "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599" => Some("wrapped-bitcoin"),
            "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2" => Some("ethereum"),
            "0x1f9840a85d5af5bf1d1762f925bdaddc4201f984" => Some("uniswap"),
            "0x514910771af9ca656af840dff83e8264ecf986ca" => Some("chainlink"),
            "0x7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9" => Some("aave"),
            _ => None,
        };

        if let Some(id) = coingecko_id {
            let api_key = std::env::var("COINGECKO_API_KEY").ok();
            let url = if let Some(key) = api_key {
                format!("https://api.coingecko.com/api/v3/coins/{}?x_cg_demo_api_key={}", id, key)
            } else {
                format!("https://api.coingecko.com/api/v3/coins/{}", id)
            };
            
            let response: Value = reqwest::get(&url).await?.json().await?;
            
            let name = response.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
            let logo_uri = response.get("image").and_then(|v| v.as_str()).map(|s| s.to_string());
            
            return Ok((name, logo_uri));
        }

        Ok((None, None))
    }
}

