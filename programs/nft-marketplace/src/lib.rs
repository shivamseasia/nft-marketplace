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
    pub fn list_nft(ctx: Context<ListNft>, price: u64) -> Result<()> {
        require!(price > 0, MarketplaceError::InvalidPrice);

        let metadata = MplMetadata::try_from(&ctx.accounts.metadata.to_account_info())
            .map_err(|_| MarketplaceError::InvalidMetadata)?;

        require!(
            metadata.mint == ctx.accounts.nft_mint.key(),
            MarketplaceError::InvalidMetadata
        );

        let listing = &mut ctx.accounts.listing;
        listing.seller = ctx.accounts.seller.key();
        listing.nft_mint = ctx.accounts.nft_mint.key();
        listing.price = price;
        listing.bump = ctx.bumps.listing;

        token::transfer(ctx.accounts.into_transfer_to_escrow(), 1)?;

        Ok(())
    }

    // ---------------- BUY NFT ----------------
    pub fn buy_nft(ctx: Context<BuyNft>) -> Result<()> {
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

    // ---------------- CANCEL ----------------
    pub fn cancel_listing(ctx: Context<CancelListing>) -> Result<()> {
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
        space = 8 + 32 + 2 + 1
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
