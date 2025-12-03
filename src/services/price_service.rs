use anyhow::Result;
use crate::services::cache::CacheService;

#[derive(Clone)]
pub struct PriceService {
    cache: CacheService,
}

impl PriceService {
    pub fn new(cache: CacheService) -> Self {
        Self { cache }
    }

    pub async fn get_solana_price(&self, token_id: &str) -> Result<f64> {
        if let Some((price, _)) = self.cache.get_price_with_change(token_id, "solana").await? {
            return Ok(price);
        }

        let (price, change) = self.fetch_jupiter_price(token_id).await?;
        
        let ttl_seconds = std::env::var("CACHE_TTL_SECONDS")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .unwrap_or(30);
        self.cache.set_price_with_change(token_id, "solana", price, change, ttl_seconds).await?;

        Ok(price)
    }

    pub async fn get_solana_price_with_change(&self, token_id: &str) -> Result<(f64, Option<f64>)> {
        if let Some(cached) = self.cache.get_price_with_change(token_id, "solana").await? {
            return Ok(cached);
        }

        let (price, change) = self.fetch_jupiter_price(token_id).await?;
        
        let ttl_seconds = std::env::var("CACHE_TTL_SECONDS")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .unwrap_or(30);
        self.cache.set_price_with_change(token_id, "solana", price, change, ttl_seconds).await?;

        Ok((price, change))
    }

    pub async fn get_ethereum_price(&self, token_id: &str) -> Result<f64> {
        if let Some((price, _)) = self.cache.get_price_with_change(token_id, "ethereum").await? {
            return Ok(price);
        }

        let (price, change) = self.fetch_coingecko_price(token_id).await?;
        
        let ttl_seconds = std::env::var("CACHE_TTL_SECONDS")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .unwrap_or(30);
        self.cache.set_price_with_change(token_id, "ethereum", price, change, ttl_seconds).await?;

        Ok(price)
    }

    pub async fn get_ethereum_price_with_change(&self, token_id: &str) -> Result<(f64, Option<f64>)> {
        if let Some(cached) = self.cache.get_price_with_change(token_id, "ethereum").await? {
            return Ok(cached);
        }

        let (price, change) = self.fetch_coingecko_price(token_id).await?;
        
        let ttl_seconds = std::env::var("CACHE_TTL_SECONDS")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .unwrap_or(30);
        self.cache.set_price_with_change(token_id, "ethereum", price, change, ttl_seconds).await?;

        Ok((price, change))
    }

    async fn fetch_jupiter_price(&self, token_id: &str) -> Result<(f64, Option<f64>)> {
        // For SOL, use CoinGecko
        if token_id == "SOL" || token_id == "So11111111111111111111111111111111111111112" {
            return self.fetch_coingecko_price("solana").await;
        }

        // TODO: Implement Jupiter price API call for other tokens
        // For now, return placeholder
        Ok((0.0, None))
    }

    async fn fetch_coingecko_price(&self, token_id: &str) -> Result<(f64, Option<f64>)> {
        let coingecko_id = match token_id.to_lowercase().as_str() {
            "eth" | "ethereum" => "ethereum",
            "sol" | "solana" => "solana",
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48" => "usd-coin",
            "0xdac17f958d2ee523a2206206994597c13d831ec7" => "tether",
            "0x6b175474e89094c44da98b954eedeac495271d0f" => "dai",
            "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599" => "wrapped-bitcoin",
            "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2" => "ethereum",
            "0x1f9840a85d5af5bf1d1762f925bdaddc4201f984" => "uniswap",
            "0x514910771af9ca656af840dff83e8264ecf986ca" => "chainlink",
            "0x7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9" => "aave",
            _ => return Ok((0.0, None)),
        };

        let api_key = std::env::var("COINGECKO_API_KEY").ok();
        let url = if let Some(key) = api_key {
            format!("https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd&include_24hr_change=true&x_cg_demo_api_key={}", coingecko_id, key)
        } else {
            format!("https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd&include_24hr_change=true", coingecko_id)
        };
        
        let response: serde_json::Value = reqwest::get(&url).await?.json().await?;
        
        let price = response[coingecko_id]["usd"]
            .as_f64()
            .unwrap_or(0.0);
        
        let change_24h = response[coingecko_id]["usd_24h_change"]
            .as_f64();

        Ok((price, change_24h))
    }
}

