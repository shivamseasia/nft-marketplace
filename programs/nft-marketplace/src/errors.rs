use anchor_lang::prelude::*;

#[error_code]
pub enum MarketplaceError {
    #[msg("Invalid price")]
    InvalidPrice,

    #[msg("Invalid NFT")]
    InvalidNft,

    #[msg("Invalid metadata")]
    InvalidMetadata,

    #[msg("Unauthorized")]
    Unauthorized,

    #[msg("Invalid creator")]
    InvalidCreator,

    #[msg("Royalty calculation error")]
    RoyaltyCalculationError,

    #[msg("Invalid platform fee")]
    InvalidPlatformFee,
}
