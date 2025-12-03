use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_request::TokenAccountsFilter;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::native_token::lamports_to_sol;
use solana_program_pack::Pack;
use spl_token::state::Account as TokenAccount;
use tracing::{info, error, debug};
use crate::types::portfolio::PortfolioResponse;
use crate::types::token::Token;
use crate::services::price_service::PriceService;
use crate::services::metadata_service::MetadataService;
use crate::config::Config;

#[derive(Clone)]
pub struct SolanaClient {
    rpc_url: String,
    price_service: PriceService,
    metadata_service: MetadataService,
    config: Config,
}

#[allow(deprecated)]
impl SolanaClient {
    pub fn new(rpc_url: String, price_service: PriceService, metadata_service: MetadataService, config: Config) -> Self {
        Self {
            rpc_url,
            price_service,
            metadata_service,
            config,
        }
    }

    pub async fn fetch_portfolio(&self, address: &str) -> Result<PortfolioResponse> {
        info!("🔍 Fetching Solana portfolio for address: {}", address);
        info!("🌐 Using RPC URL: {}", self.rpc_url);
        
        let pubkey = address.parse::<Pubkey>()
            .map_err(|e| {
                error!("❌ Failed to parse Solana address {}: {:?}", address, e);
                e
            })?;
        info!("✅ Parsed address: {:?}", pubkey);
        
        let rpc_client = RpcClient::new(self.rpc_url.clone());
        info!("🔌 Created RPC client");
        
        // Fetch SOL balance
        info!("💰 Fetching SOL balance for address {:?}...", pubkey);
        let lamports = rpc_client.get_balance(&pubkey)
            .map_err(|e| {
                error!("❌ Failed to get balance: {:?}", e);
                e
            })?;
        
        info!("📊 Raw balance (lamports): {}", lamports);
        #[allow(deprecated)]
        let sol_balance = lamports_to_sol(lamports);
        info!("📊 SOL balance: {} SOL", sol_balance);
        
        // Get SOL price
        info!("💵 Fetching SOL price...");
        let (sol_price, _sol_price_change) = self.price_service.get_solana_price_with_change("SOL").await.unwrap_or((0.0, None));
        info!("💵 SOL price: ${}", sol_price);
        
        let sol_value = sol_balance * sol_price;
        info!("💵 SOL value: ${}", sol_value);

        // Fetch SPL token balances
        let mut tokens: Vec<Token> = Vec::new();
        
        let token_program_id = spl_token::ID;
        info!("🪙 Fetching SPL token balances for program: {:?}...", token_program_id);
        let token_accounts = rpc_client.get_token_accounts_by_owner(
            &pubkey,
            TokenAccountsFilter::ProgramId(token_program_id),
        )
        .map_err(|e| {
            error!("❌ Failed to get token accounts: {:?}", e);
            e
        })?;
        
        info!("📊 Found {} token accounts", token_accounts.len());

        for account in token_accounts {
            let account_pubkey: Pubkey = account.pubkey.parse()?;
            debug!("🔍 Processing token account: {}", account.pubkey);
            
            if let Ok(account_info) = rpc_client.get_account(&account_pubkey) {
                if let Ok(token_account) = TokenAccount::unpack(&account_info.data) {
                    let mint = token_account.mint.to_string();
                    debug!("✅ Unpacked token account for mint: {}", mint);
                    
                    let decimals = 9u8; // Default, should fetch from mint account
                    let amount = token_account.amount as f64 / 10_f64.powi(decimals as i32);
                    
                    debug!("📊 Token amount: {} (raw: {})", amount, token_account.amount);
                    
                    if amount > 0.0 {
                        info!("💰 Found token with balance: {} (mint: {})", amount, mint);
                        
                        let (price, price_change) = self.price_service.get_solana_price_with_change(&mint).await.unwrap_or((0.0, None));
                        let value = amount * price;
                        
                        debug!("💵 Token price: ${}, value: ${}", price, value);
                        
                        let (name, logo_uri) = self.metadata_service.get_solana_metadata(&mint).await.unwrap_or((None, None));
                        
                        let symbol = if let Some(ref n) = name {
                            n.split_whitespace().next().unwrap_or(&mint[..8]).to_string()
                        } else {
                            mint.chars().take(8).collect()
                        };

                        tokens.push(Token {
                            symbol: symbol.clone(),
                            mint_or_address: mint,
                            amount,
                            decimals,
                            price_usd: price,
                            value_usd: value,
                            name,
                            logo_uri,
                            price_change_24h: price_change,
                        });
                        
                        info!("✅ Added token: {} ({}), value: ${}", symbol, amount, value);
                    }
                } else {
                    debug!("⚠️ Failed to unpack token account for: {}", account.pubkey);
                }
            } else {
                debug!("⚠️ Failed to get account info for: {}", account.pubkey);
            }
        }
        
        info!("🪙 Found {} SPL tokens with balance > 0", tokens.len());

        let total_tokens_count = tokens.len() + if sol_balance > 0.0 { 1 } else { 0 };
        let last_updated = chrono::Utc::now().to_rfc3339();

        let portfolio = PortfolioResponse {
            chain: "solana".to_string(),
            address: address.to_string(),
            native_balance: sol_balance,
            native_price_usd: sol_price,
            native_value_usd: sol_value,
            tokens: tokens.clone(),
            total_tokens_count: Some(total_tokens_count),
            last_updated: Some(last_updated),
        };
        
        info!("📦 Portfolio Response:");
        info!("   - Chain: {}", portfolio.chain);
        info!("   - Address: {}", portfolio.address);
        info!("   - Native Balance: {} SOL", portfolio.native_balance);
        info!("   - Native Price: ${}", portfolio.native_price_usd);
        info!("   - Native Value: ${}", portfolio.native_value_usd);
        info!("   - Total Tokens: {:?}", portfolio.total_tokens_count);
        info!("   - SPL Tokens: {}", portfolio.tokens.len());
        
        Ok(portfolio)
    }
}

