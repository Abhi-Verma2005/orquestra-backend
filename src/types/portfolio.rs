use serde::{Deserialize, Serialize};
use crate::types::token::Token;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioResponse {
    pub chain: String,
    pub address: String,
    #[serde(rename = "nativeBalance")]
    pub native_balance: f64,
    #[serde(rename = "nativePriceUsd")]
    pub native_price_usd: f64,
    #[serde(rename = "nativeValueUsd")]
    pub native_value_usd: f64,
    pub tokens: Vec<Token>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "totalTokensCount")]
    pub total_tokens_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastUpdated")]
    pub last_updated: Option<String>,
}

