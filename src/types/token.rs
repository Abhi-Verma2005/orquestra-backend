use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub symbol: String,
    #[serde(rename = "mintOrAddress")]
    pub mint_or_address: String,
    pub amount: f64,
    pub decimals: u8,
    #[serde(rename = "priceUsd")]
    pub price_usd: f64,
    #[serde(rename = "valueUsd")]
    pub value_usd: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "logoUri")]
    pub logo_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "priceChange24h")]
    pub price_change_24h: Option<f64>,
}

