use anyhow::Result;
use crate::types::portfolio::PortfolioResponse;
use crate::services::ethereum_client::EthereumClient;

pub async fn fetch_ethereum_balance(
    ethereum_client: &EthereumClient,
    address: &str,
) -> Result<PortfolioResponse> {
    ethereum_client.fetch_portfolio(address).await
}

