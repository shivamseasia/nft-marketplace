use anchor_lang::prelude::*;
use anchor_spl::metadata::mpl_token_metadata::accounts::Metadata as MplMetadata;
use anchor_spl::metadata::{self};
use anchor_spl::token::{self, CloseAccount, Mint, Token, TokenAccount, Transfer};

mod constants;
mod errors;
mod state;

use constants::*;
use errors::*;
use state::*;

declare_id!("77565CzCRLiTaNudYKXSwVPuwQpBQuKPQsr8vBifVX4k");

#[program]
pub mod nft_marketplace {
    use super::*;

    // ---------------- INITIALIZE MARKETPLACE ----------------
    pub fn initialize_marketplace(
        ctx: Context<InitializeMarketplace>,
        platform_fee_bps: u16,
    ) -> Result<()> {
        require!(
            !ctx.accounts.marketplace.paused,
            MarketplaceError::MarketplacePaused
        );

        require!(
            platform_fee_bps <= 1000,
            MarketplaceError::InvalidPlatformFee
        );

        let marketplace = &mut ctx.accounts.marketplace;
        marketplace.authority = ctx.accounts.admin.key();
        marketplace.platform_fee_bps = platform_fee_bps;
        marketplace.bump = ctx.bumps.marketplace;

        Ok(())
    }

    // ---------------- LIST NFT ----------------
    pub fn list_nft(ctx: Context<ListNft>, price: u64, payment_mint: Pubkey) -> Result<()> {
        require!(
            !ctx.accounts.marketplace.paused,
            MarketplaceError::MarketplacePaused
        );

        require!(price > 0, MarketplaceError::InvalidPrice);

        let metadata = MplMetadata::try_from(&ctx.accounts.metadata.to_account_info())
            .map_err(|_| MarketplaceError::InvalidMetadata)?;
        let collection = metadata
            .collection
            .ok_or(MarketplaceError::InvalidCollection)?;

        let whitelist = &ctx.accounts.whitelisted_collection;

        require!(
            metadata.mint == ctx.accounts.nft_mint.key(),
            MarketplaceError::InvalidMetadata
        );

        require!(collection.verified, MarketplaceError::InvalidCollection);
        require!(
            whitelist.collection_mint == collection.key,
            MarketplaceError::CollectionNotWhitelisted
        );

        require!(
            payment_mint == Pubkey::default() || payment_mint == USDC_MINT,
            MarketplaceError::InvalidPaymentMint
        );

        let listing = &mut ctx.accounts.listing;
        listing.seller = ctx.accounts.seller.key();
        listing.nft_mint = ctx.accounts.nft_mint.key();
        listing.price = price;
        listing.payment_mint = payment_mint;
        listing.bump = ctx.bumps.listing;

        token::transfer(ctx.accounts.into_transfer_to_escrow(), 1)?;

        Ok(())
    }

    // ---------------- BUY NFT ----------------
    pub fn buy_nft(ctx: Context<BuyNft>) -> Result<()> {
        require!(
            !ctx.accounts.marketplace.paused,
            MarketplaceError::MarketplacePaused
        );

        let listing = &ctx.accounts.listing;
        if listing.payment_mint == Pubkey::default() {
            process_sol_purchase(ctx)
        } else {
            process_usdc_purchase(ctx)
        }
    }

    // ---------------- CANCEL ----------------
    pub fn cancel_listing(ctx: Context<CancelListing>) -> Result<()> {
        require!(
            !ctx.accounts.marketplace.paused,
            MarketplaceError::MarketplacePaused
        );
        let listing = &ctx.accounts.listing;

        require!(
            ctx.accounts.seller.key() == listing.seller,
            MarketplaceError::Unauthorized
        );

        let seeds = &[LISTING_SEED, listing.nft_mint.as_ref(), &[listing.bump]];
        let signer = &[&seeds[..]];

        token::transfer(
            ctx.accounts
                .into_transfer_back_to_seller()
                .with_signer(signer),
            1,
        )?;

        token::close_account(ctx.accounts.into_close_escrow().with_signer(signer))?;

        Ok(())
    }

    pub fn add_collection(ctx: Context<AddCollection>) -> Result<()> {
        require!(
            !ctx.accounts.marketplace.paused,
            MarketplaceError::MarketplacePaused
        );

        let collection = &mut ctx.accounts.collection;
        collection.collection_mint = ctx.accounts.collection_mint.key();
        collection.bump = ctx.bumps.collection;

        Ok(())
    }

    pub fn remove_collection(ctx: Context<RemoveCollection>) -> Result<()> {
        require!(
            !ctx.accounts.marketplace.paused,
            MarketplaceError::MarketplacePaused
        );
        Ok(())
    }

    pub fn pause_marketplace(ctx: Context<AdminAction>) -> Result<()> {
        let marketplace = &mut ctx.accounts.marketplace;

        require!(
            marketplace.authority == ctx.accounts.authority.key(),
            MarketplaceError::UnauthorizedAdmin
        );

        marketplace.paused = true;
        Ok(())
    }

    pub fn unpause_marketplace(ctx: Context<AdminAction>) -> Result<()> {
        let marketplace = &mut ctx.accounts.marketplace;

        require!(
            marketplace.authority == ctx.accounts.authority.key(),
            MarketplaceError::UnauthorizedAdmin
        );

        marketplace.paused = false;
        Ok(())
    }

    pub fn update_platform_fee(ctx: Context<AdminAction>, new_fee_bps: u16) -> Result<()> {
        require!(new_fee_bps <= 1000, MarketplaceError::InvalidPlatformFee);

        let marketplace = &mut ctx.accounts.marketplace;

        require!(
            marketplace.authority == ctx.accounts.authority.key(),
            MarketplaceError::UnauthorizedAdmin
        );

        marketplace.platform_fee_bps = new_fee_bps;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeMarketplace<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        init,
        payer = admin,
        seeds = [MARKETPLACE_SEED],
        bump,
        space = 8 + 32 + 2 + 1 + 1
    )]
    pub marketplace: Account<'info, MarketplaceConfig>,

    /// Treasury PDA to hold platform fees (SOL)
    #[account(
        init,
        payer = admin,
        seeds = [TREASURY_SEED],
        bump,
        space = 0
    )]
    pub treasury: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ListNft<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,

    #[account(
        constraint = nft_mint.decimals == 0 @ MarketplaceError::InvalidNft,
        constraint = nft_mint.supply == 1 @ MarketplaceError::InvalidNft
    )]
    pub nft_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = seller_nft_account.owner == seller.key(),
        constraint = seller_nft_account.amount == 1
    )]
    pub seller_nft_account: Account<'info, TokenAccount>,

    /// CHECK: Metaplex Metadata PDA
    #[account(
        seeds = [
            b"metadata",
            metadata::ID.as_ref(),
            nft_mint.key().as_ref(),
        ],
        seeds::program = metadata::ID,
        bump,
    )]
    pub metadata: UncheckedAccount<'info>,

    /// CHECK: Master Edition PDA
    #[account(
        seeds = [
            b"metadata",
            metadata::ID.as_ref(),
            nft_mint.key().as_ref(),
            b"edition",
        ],
        seeds::program = metadata::ID,
        bump,
    )]
    pub master_edition: UncheckedAccount<'info>,

    #[account(
        seeds = [COLLECTION_SEED, collection_mint.key().as_ref()],
        bump
    )]
    pub whitelisted_collection: Account<'info, WhitelistedCollection>,

    pub collection_mint: Account<'info, Mint>,

    #[account(
    seeds = [MARKETPLACE_SEED],
    bump = marketplace.bump
)]
    pub marketplace: Account<'info, MarketplaceConfig>,

    #[account(
        init,
        payer = seller,
        seeds = [LISTING_SEED, nft_mint.key().as_ref()],
        bump,
        space = 8 + 32 + 32 + 8 + 1
    )]
    pub listing: Account<'info, Listing>,

    #[account(
        init,
        payer = seller,
        seeds = [ESCROW_SEED, nft_mint.key().as_ref()],
        bump,
        token::mint = nft_mint,
        token::authority = listing
    )]
    pub escrow_nft_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

impl<'info> ListNft<'info> {
    fn into_transfer_to_escrow(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.seller_nft_account.to_account_info(),
                to: self.escrow_nft_account.to_account_info(),
                authority: self.seller.to_account_info(),
            },
        )
    }
}

#[derive(Accounts)]
pub struct BuyNft<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,

    #[account(mut)]
    pub seller: SystemAccount<'info>,

    pub nft_mint: Account<'info, Mint>,

    /// CHECK: Metaplex Metadata PDA
    #[account(
        seeds = [
            b"metadata",
            metadata::ID.as_ref(),
            nft_mint.key().as_ref(),
        ],
        seeds::program = metadata::ID,
        bump,
    )]
    pub metadata: UncheckedAccount<'info>,

    /// Marketplace config PDA (v4)
    #[account(
        seeds = [MARKETPLACE_SEED],
        bump = marketplace.bump
    )]
    pub marketplace: Account<'info, MarketplaceConfig>,

    /// Treasury PDA (SOL holder)
    #[account(
        mut,
        seeds = [TREASURY_SEED],
        bump
    )]
    pub treasury: SystemAccount<'info>,

    // ---------- USDC accounts ----------
    #[account(
        mut,
        constraint = buyer_usdc.mint == USDC_MINT
    )]
    pub buyer_usdc: Option<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = seller_usdc.mint == USDC_MINT
    )]
    pub seller_usdc: Option<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = treasury_usdc.mint == USDC_MINT
    )]
    pub treasury_usdc: Option<Account<'info, TokenAccount>>,
    // ----------------------------------
    #[account(
        mut,
        seeds = [LISTING_SEED, nft_mint.key().as_ref()],
        bump = listing.bump,
        close = seller
    )]
    pub listing: Account<'info, Listing>,

    #[account(
        mut,
        seeds = [ESCROW_SEED, nft_mint.key().as_ref()],
        bump,
    )]
    pub escrow_nft_account: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = buyer,
        token::mint = nft_mint,
        token::authority = buyer
    )]
    pub buyer_nft_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

impl<'info> BuyNft<'info> {
    fn into_transfer_to_buyer(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.escrow_nft_account.to_account_info(),
                to: self.buyer_nft_account.to_account_info(),
                authority: self.listing.to_account_info(),
            },
        )
    }

    fn into_close_escrow(&self) -> CpiContext<'_, '_, '_, 'info, CloseAccount<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            CloseAccount {
                account: self.escrow_nft_account.to_account_info(),
                destination: self.seller.to_account_info(),
                authority: self.listing.to_account_info(),
            },
        )
    }

    fn into_transfer_to_treasury(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.buyer_usdc.as_ref().unwrap().to_account_info(),
                to: self.treasury_usdc.as_ref().unwrap().to_account_info(),
                authority: self.buyer.to_account_info(),
            },
        )
    }

    fn into_transfer_to_seller(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.buyer_usdc.as_ref().unwrap().to_account_info(),
                to: self.seller_usdc.as_ref().unwrap().to_account_info(),
                authority: self.buyer.to_account_info(),
            },
        )
    }
}

#[derive(Accounts)]
pub struct CancelListing<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,

    pub nft_mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [LISTING_SEED, nft_mint.key().as_ref()],
        bump = listing.bump,
        close = seller
    )]
    pub listing: Account<'info, Listing>,

    #[account(
        mut,
        seeds = [ESCROW_SEED, nft_mint.key().as_ref()],
        bump,
    )]
    pub escrow_nft_account: Account<'info, TokenAccount>,

    #[account(
    seeds = [MARKETPLACE_SEED],
    bump = marketplace.bump
)]
    pub marketplace: Account<'info, MarketplaceConfig>,

    #[account(
        mut,
        constraint = seller_nft_account.owner == seller.key()
    )]
    pub seller_nft_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

impl<'info> CancelListing<'info> {
    fn into_transfer_back_to_seller(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.escrow_nft_account.to_account_info(),
                to: self.seller_nft_account.to_account_info(),
                authority: self.listing.to_account_info(),
            },
        )
    }

    fn into_close_escrow(&self) -> CpiContext<'_, '_, '_, 'info, CloseAccount<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            CloseAccount {
                account: self.escrow_nft_account.to_account_info(),
                destination: self.seller.to_account_info(),
                authority: self.listing.to_account_info(),
            },
        )
    }
}

#[derive(Accounts)]
pub struct AddCollection<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MARKETPLACE_SEED],
        bump = marketplace.bump,
        constraint = marketplace.authority == authority.key()
    )]
    pub marketplace: Account<'info, MarketplaceConfig>,

    pub collection_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = authority,
        seeds = [COLLECTION_SEED, collection_mint.key().as_ref()],
        bump,
        space = 8 + 32 + 1
    )]
    pub collection: Account<'info, WhitelistedCollection>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RemoveCollection<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [MARKETPLACE_SEED],
        bump = marketplace.bump,
        constraint = marketplace.authority == authority.key()
    )]
    pub marketplace: Account<'info, MarketplaceConfig>,

    #[account(
        mut,
        close = authority,
        seeds = [COLLECTION_SEED, collection.collection_mint.as_ref()],
        bump = collection.bump
    )]
    pub collection: Account<'info, WhitelistedCollection>,
}

#[derive(Accounts)]
pub struct AdminAction<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [MARKETPLACE_SEED],
        bump = marketplace.bump
    )]
    pub marketplace: Account<'info, MarketplaceConfig>,
}

// ---------------- INTERNAL ROYALTY HELPER ----------------

fn distribute_royalties(
    metadata: &MplMetadata,
    price: u64,
    buyer: &AccountInfo,
    creator_accounts: &[AccountInfo],
) -> Result<u64> {
    let royalty_bps = metadata.seller_fee_basis_points as u64;
    if royalty_bps == 0 {
        return Ok(price);
    }

    let total_royalty = price
        .checked_mul(royalty_bps)
        .ok_or(MarketplaceError::RoyaltyCalculationError)?
        .checked_div(10_000)
        .ok_or(MarketplaceError::RoyaltyCalculationError)?;

    let creators = metadata
        .creators
        .as_ref()
        .ok_or(MarketplaceError::InvalidCreator)?;

    let mut paid = 0u64;
    let mut idx = 0usize;

    for creator in creators.iter().filter(|c| c.verified) {
        let share = (total_royalty * creator.share as u64) / 100;
        let creator_account = creator_accounts
            .get(idx)
            .ok_or(MarketplaceError::InvalidCreator)?;

        require!(
            creator_account.key() == creator.address,
            MarketplaceError::InvalidCreator
        );

        **buyer.try_borrow_mut_lamports()? -= share;
        **creator_account.try_borrow_mut_lamports()? += share;

        paid += share;
        idx += 1;
    }

    Ok(price - paid)
}

fn process_sol_purchase(ctx: Context<BuyNft>) -> Result<()> {
    let listing = &ctx.accounts.listing;
    let marketplace = &ctx.accounts.marketplace;

    let metadata = MplMetadata::try_from(&ctx.accounts.metadata.to_account_info())
        .map_err(|_| MarketplaceError::InvalidMetadata)?;

    // -------- PLATFORM FEE --------
    let platform_fee = (listing.price as u128)
        .checked_mul(marketplace.platform_fee_bps as u128)
        .unwrap()
        .checked_div(10_000)
        .unwrap() as u64;

    **ctx
        .accounts
        .buyer
        .to_account_info()
        .try_borrow_mut_lamports()? -= platform_fee;
    **ctx
        .accounts
        .treasury
        .to_account_info()
        .try_borrow_mut_lamports()? += platform_fee;

    let remaining_price = listing.price - platform_fee;

    // -------- ROYALTIES --------
    let seller_amount = distribute_royalties(
        &metadata,
        remaining_price,
        &ctx.accounts.buyer.to_account_info(),
        &ctx.remaining_accounts,
    )?;

    // -------- PAY SELLER --------
    **ctx
        .accounts
        .buyer
        .to_account_info()
        .try_borrow_mut_lamports()? -= seller_amount;
    **ctx
        .accounts
        .seller
        .to_account_info()
        .try_borrow_mut_lamports()? += seller_amount;

    let seeds = &[LISTING_SEED, listing.nft_mint.as_ref(), &[listing.bump]];
    let signer = &[&seeds[..]];

    token::transfer(ctx.accounts.into_transfer_to_buyer().with_signer(signer), 1)?;

    token::close_account(ctx.accounts.into_close_escrow().with_signer(signer))?;

    Ok(())
}

fn process_usdc_purchase(ctx: Context<BuyNft>) -> Result<()> {
    let listing = &ctx.accounts.listing;
    let marketplace = &ctx.accounts.marketplace;

    let price = listing.price;
    let platform_fee = price * marketplace.platform_fee_bps as u64 / 10_000;

    // Transfer platform fee
    token::transfer(ctx.accounts.into_transfer_to_treasury(), platform_fee)?;

    // Distribute royalties (same logic as v3, token-based)
    let seller_amount = distribute_royalties_token(
        price - platform_fee,
        &ctx.accounts.buyer_usdc.as_ref().unwrap().to_account_info(),
        &ctx.remaining_accounts,
        &ctx.accounts.token_program,
    )?;

    // Pay seller
    token::transfer(ctx.accounts.into_transfer_to_seller(), seller_amount)?;

    // Transfer NFT
    token::transfer(ctx.accounts.into_transfer_to_buyer(), 1)?;

    Ok(())
}

fn distribute_royalties_token(
    amount: u64,
    payer: &AccountInfo,
    remaining_accounts: &[AccountInfo],
    token_program: &Program<Token>,
) -> Result<u64> {
    if remaining_accounts.is_empty() {
        return Ok(amount);
    }

    let mut total_royalty: u64 = 0;

    for chunk in remaining_accounts.chunks(2) {
        let creator_token_account = &chunk[0];
        let creator_share_account = &chunk[1];

        let share = creator_share_account
            .try_borrow_data()?
            .get(0)
            .copied()
            .unwrap_or(0) as u64;

        let royalty_amount = amount * share / 100;
        total_royalty += royalty_amount;

        token::transfer(
            CpiContext::new(
                token_program.to_account_info(),
                Transfer {
                    from: payer.clone(),
                    to: creator_token_account.clone(),
                    authority: payer.clone(),
                },
            ),
            royalty_amount,
        )?;
    }

    Ok(amount - total_royalty)
}

