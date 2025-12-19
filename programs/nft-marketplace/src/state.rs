use anchor_lang::prelude::*;

#[account]
pub struct Listing {
    pub seller: Pubkey,
    pub nft_mint: Pubkey,
    pub price: u64, // lamports
    pub bump: u8,
}

#[account]
pub struct MarketplaceConfig {
    pub authority: Pubkey,
    pub platform_fee_bps: u16,
    pub bump: u8,
}
