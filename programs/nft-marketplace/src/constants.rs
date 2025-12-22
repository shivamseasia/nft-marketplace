use anchor_lang::prelude::*;

pub const LISTING_SEED: &[u8] = b"listing";
pub const ESCROW_SEED: &[u8] = b"escrow";

// Metaplex Token Metadata Program ID
pub const METADATA_PROGRAM_ID: Pubkey =
    pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");


pub const MARKETPLACE_SEED: &[u8] = b"marketplace";
pub const TREASURY_SEED: &[u8] = b"treasury";

pub const MAX_PLATFORM_FEE_BPS: u16 = 1_000; // 10%
pub const COLLECTION_SEED: &[u8] = b"collection";