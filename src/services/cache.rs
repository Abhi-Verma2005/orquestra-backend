use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{Utc, DateTime};

#[derive(Clone)]
struct CacheEntry {
    data: Value,
    expires_at: DateTime<chrono::Utc>,
}

#[derive(Clone)]
pub struct CacheService {
    prices: Arc<RwLock<HashMap<String, (f64, Option<f64>, DateTime<chrono::Utc>)>>>,
    metadata: Arc<RwLock<HashMap<String, (Value, DateTime<chrono::Utc>)>>>,
    balances: Arc<RwLock<HashMap<String, (Value, DateTime<chrono::Utc>)>>>,
}

impl CacheService {
    pub fn new() -> Self {
        Self {
            prices: Arc::new(RwLock::new(HashMap::new())),
            metadata: Arc::new(RwLock::new(HashMap::new())),
            balances: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_price_with_change(&self, token_id: &str, chain: &str) -> Result<Option<(f64, Option<f64>)>> {
        let key = format!("{}:{}", chain, token_id);
        let prices = self.prices.read().await;
        
        if let Some((price, change, expires_at)) = prices.get(&key) {
            if *expires_at > Utc::now() {
                return Ok(Some((*price, *change)));
            }
        }
        
        Ok(None)
    }

    pub async fn set_price_with_change(&self, token_id: &str, chain: &str, price: f64, change: Option<f64>, ttl_seconds: u64) -> Result<()> {
        let key = format!("{}:{}", chain, token_id);
        let expires_at = Utc::now() + chrono::Duration::seconds(ttl_seconds as i64);
        let mut prices = self.prices.write().await;
        prices.insert(key, (price, change, expires_at));
        Ok(())
    }

    pub async fn get_metadata(&self, address: &str, chain: &str) -> Result<Option<Value>> {
        let key = format!("{}:{}", chain, address);
        let metadata = self.metadata.read().await;
        
        if let Some((value, expires_at)) = metadata.get(&key) {
            if *expires_at > Utc::now() {
                return Ok(Some(value.clone()));
            }
        }
        
        Ok(None)
    }

    pub async fn set_metadata(&self, address: &str, chain: &str, data: &Value, ttl_seconds: u64) -> Result<()> {
        let key = format!("{}:{}", chain, address);
        let expires_at = Utc::now() + chrono::Duration::seconds(ttl_seconds as i64);
        let mut metadata = self.metadata.write().await;
        metadata.insert(key, (data.clone(), expires_at));
        Ok(())
    }

    pub async fn get_balance(&self, address: &str, chain: &str) -> Result<Option<Value>> {
        let key = format!("{}:{}", chain, address);
        let balances = self.balances.read().await;
        
        if let Some((value, expires_at)) = balances.get(&key) {
            if *expires_at > Utc::now() {
                return Ok(Some(value.clone()));
            }
        }
        
        Ok(None)
    }

    pub async fn set_balance(&self, address: &str, chain: &str, data: &Value, ttl_seconds: u64) -> Result<()> {
        let key = format!("{}:{}", chain, address);
        let expires_at = Utc::now() + chrono::Duration::seconds(ttl_seconds as i64);
        let mut balances = self.balances.write().await;
        balances.insert(key, (data.clone(), expires_at));
        Ok(())
    }
}

