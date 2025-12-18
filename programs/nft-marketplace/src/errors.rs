use anchor_lang::prelude::*;

#[error_code]
pub enum MarketplaceError {
    #[msg("Price must be greater than zero")]
    InvalidPrice,

    #[msg("You are not the seller")]
    Unauthorized,

    #[msg("Invalid NFT")]
    InvalidNft,
}
