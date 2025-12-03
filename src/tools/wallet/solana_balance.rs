use anyhow::Result;
use crate::types::portfolio::PortfolioResponse;
use crate::services::solana_client::SolanaClient;

pub async fn fetch_solana_balance(
    solana_client: &SolanaClient,
    address: &str,
) -> Result<PortfolioResponse> {
    solana_client.fetch_portfolio(address).await
}

