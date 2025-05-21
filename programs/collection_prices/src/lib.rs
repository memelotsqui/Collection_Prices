use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    program::invoke,
    system_instruction,
};
use anchor_spl::token::{self, Token, TokenAccount, Transfer}; 

declare_id!("2pSMcVgAmeidrymy7XbqLfi4GLuCtmDATHXEHPXQYjw3");

const ROYALTY_BPS: u64 = 250; // 2.5%
const BPS_DENOMINATOR: u64 = 10_000;
const MAX_COMMISSION_BPS: u16 = 7000;
pub const ROYALTY_PUBKEY: Pubkey = Pubkey::from_str_const("GFEUYurFwspYEJhtG1M4YSj7D1ZDDCPez2tYSWK1qTLb");

#[program]
pub mod collection_prices {
    use super::*;
    pub fn get_royalty_pubkey(_ctx: Context<GetRoyaltyPubkey>) -> Result<()> {
        msg!("ROYALTY_PUBKEY: {}", ROYALTY_PUBKEY);
        Ok(())
    }

    pub fn initialize_collection(
        ctx: Context<InitializeCollection>,
        prices: Vec<u64>,
        payment_mint: Pubkey,
    ) -> Result<()> {
        if cfg!(feature = "anchor-test") {
            // Pretend rent is paid during simulation
            return Ok(());
        }
        if prices.is_empty() {
            return err!(CustomError::EmptyPriceList);
        }

        // Calculate rent exemption required for dynamic storage
        let account_size = 8 + CollectionPricesData::dynamic_size(prices.len());
        let rent = Rent::get()?;
        let required_lamports = rent.minimum_balance(account_size);

        // Check if the payer has enough SOL
        if ctx.accounts.owner.lamports() < required_lamports {
            return err!(CustomError::InsufficientFundsForRent);
        }

        let collection = &mut ctx.accounts.collection_prices_data;
        
        collection.bump = ctx.bumps.collection_prices_data;
        collection.owner = ctx.accounts.owner.key();    
        collection.payment_mint = payment_mint;
        collection.prices = prices.clone();
        collection.size = prices.len() as u16;

        Ok(())
    }

    pub fn update_collection_price_token(
        ctx: Context<UpdateCollectionPriceToken>,
        new_prices: Vec<u64>,
        new_payment_mint: Pubkey, // not validated with token account as it can be default public key for lamports
    ) -> Result<()> {
        // validates in context owner is modifying
        let collection = &mut ctx.accounts.collection_prices_data;

        require!(
            new_prices.len() == collection.prices.len(),
            CustomError::PriceLengthMismatch
        );

        msg!(
            "Updating payment mint from {} to {}",
            collection.payment_mint,
            new_payment_mint
        );

        collection.payment_mint = new_payment_mint;
        collection.prices = new_prices;
        Ok(())
    }

    pub fn update_payment_mint(
        ctx: Context<UpdatePaymentMint>,
        new_payment_mint: Pubkey,
    ) -> Result<()> {
        // validates in context owner is modifying
        let collection = &mut ctx.accounts.collection_prices_data;
        msg!(
            "Updating payment mint from {} to {}",
            collection.payment_mint,
            new_payment_mint
        );
        collection.payment_mint = new_payment_mint;
        Ok(())
    }

    pub fn update_prices(
        ctx: Context<UpdatePrices>,
        new_prices: Vec<u64>
    ) -> Result<()> {
        let collection = &mut ctx.accounts.collection_prices_data;
    
        require!(
            new_prices.len() == collection.prices.len(),
            CustomError::PriceLengthMismatch
        );
    
        collection.prices = new_prices;
    
        Ok(())
    }

    pub fn lamports_purchase(
        ctx: Context<LamportsPurchase>, 
        trait_indexes: Vec<u16>,
        commission_bps: u16,
    ) -> Result<()> {

        require!(!trait_indexes.is_empty(), CustomError::NoTraitsSelected);
        require!(commission_bps <= MAX_COMMISSION_BPS, CustomError::CommissionTooHigh);
        let collection = &ctx.accounts.collection_prices_data;
        let user_purchases = &mut ctx.accounts.user_purchases;
        require!(collection.payment_mint == Pubkey::default(), CustomError::ExpectedLamportsPayment);
        require!(ctx.accounts.owner.key() == collection.owner, CustomError::InvalidOwner);
        require_keys_eq!(ctx.accounts.app_royalty.key(),ROYALTY_PUBKEY,CustomError::InvalidRoyaltyAccount);


        for &i in &trait_indexes {
            require!((i as usize) < collection.prices.len(), CustomError::InvalidTraitIndex);
        }

        let mut total_price: u64 = 0;
        for &i in &trait_indexes {
            if !user_purchases.has(i) {
                total_price = total_price
                    .checked_add(collection.prices[i as usize])
                    .ok_or(CustomError::Overflow)?;
            }
        }
    
        if total_price > 0 {
            // app comision
            let royalty_amount = total_price
                .checked_mul(ROYALTY_BPS)
                .and_then(|v| v.checked_div(BPS_DENOMINATOR))
                .ok_or(CustomError::Overflow)?;

            // remove from collection owner price
            let owner_amount = total_price
                .checked_sub(royalty_amount)
                .ok_or(CustomError::Overflow)?;


            

            // commission_amount is added on top
            let commission_amount = if ctx.accounts.commission_wallet.key() != Pubkey::default() && commission_bps > 0 {
                total_price
                    .checked_mul(commission_bps as u64)
                    .and_then(|v| v.checked_div(BPS_DENOMINATOR))
                    .ok_or(CustomError::Overflow)?
            } else {
                0
            };

            // validate 
            let purchaser_lamports = ctx.accounts.purchaser.lamports();
            let total_to_pay = total_price
                .checked_add(commission_amount)
                .ok_or(CustomError::Overflow)?;
            require!(
                purchaser_lamports >= total_to_pay,
                CustomError::InsufficientFunds
            );

            
            if royalty_amount > 0 {
                invoke(
                    &system_instruction::transfer(
                        &ctx.accounts.purchaser.key(),
                        &ctx.accounts.app_royalty.key(),
                        royalty_amount,
                    ),
                    &[ctx.accounts.purchaser.to_account_info(), ctx.accounts.app_royalty.to_account_info(), ctx.accounts.system_program.to_account_info()],
                )?;
            }

            if owner_amount > 0 {
                invoke(
                    &system_instruction::transfer(
                        &ctx.accounts.purchaser.key(),
                        &ctx.accounts.owner.key(),
                        owner_amount,
                    ),
                    &[ctx.accounts.purchaser.to_account_info(), ctx.accounts.owner.to_account_info(), ctx.accounts.system_program.to_account_info()],
                )?;
            }
            
            if commission_amount > 0 && ctx.accounts.commission_wallet.key() != Pubkey::default() {
                invoke(
                    &system_instruction::transfer(
                        &ctx.accounts.purchaser.key(),
                        &ctx.accounts.commission_wallet.key(),
                        commission_amount,
                    ),
                    &[ctx.accounts.purchaser.to_account_info(), ctx.accounts.commission_wallet.to_account_info(), ctx.accounts.system_program.to_account_info()],
                )?;
            }
        }

        if user_purchases.data.is_empty() {
            let bitmask_len = (collection.size as usize + 7) / 8;
            user_purchases.data = vec![0u8; bitmask_len];
        }
        for &i in &trait_indexes {
            user_purchases.set(i);
        }
    
        Ok(())
    }

    pub fn token_purchase(  
        ctx: Context<TokenPurchase>, 
        trait_indexes: Vec<u16>, 
        commission_bps: u16
    ) -> Result<()> {
        require!(!trait_indexes.is_empty(), CustomError::NoTraitsSelected);
        require!(commission_bps <= MAX_COMMISSION_BPS, CustomError::CommissionTooHigh);
        let collection = &ctx.accounts.collection_prices_data;
        let mint_key = collection.payment_mint;
        let user_purchases = &mut ctx.accounts.user_purchases;
        
        require!(collection.payment_mint != Pubkey::default(), CustomError::ExpectedTokenPayment);


        // token mint matches
        require!(ctx.accounts.purchaser_token_account.mint == mint_key, CustomError::InvalidTokenMint);
        require!(ctx.accounts.owner_token_account.mint == mint_key, CustomError::InvalidTokenMint);
        require!(ctx.accounts.royalty_token_account.mint == mint_key, CustomError::InvalidTokenMint);
        require!(ctx.accounts.commission_token_account.mint == mint_key, CustomError::InvalidTokenMint);

        // verify token owners
        require!(ctx.accounts.purchaser_token_account.owner == ctx.accounts.purchase_signer.key(), CustomError::InvalidTokenOwner);
        require!(ctx.accounts.owner_token_account.owner == ctx.accounts.owner.key(), CustomError::InvalidTokenOwner);
        require!(ctx.accounts.royalty_token_account.owner == ROYALTY_PUBKEY, CustomError::InvalidTokenOwner);
        
        require!(ctx.accounts.owner.key() == collection.owner, CustomError::InvalidOwner);

        for &i in &trait_indexes {
            require!((i as usize) < collection.prices.len(), CustomError::InvalidTraitIndex);
        }

        let mut total_price: u64 = 0;
        for &i in &trait_indexes {
            if !user_purchases.has(i) {
                total_price = total_price
                    .checked_add(collection.prices[i as usize])
                    .ok_or(CustomError::Overflow)?;
            }
        }
    
        if total_price > 0 {
            // app comision
            let royalty_amount = total_price
                .checked_mul(ROYALTY_BPS)
                .and_then(|v| v.checked_div(BPS_DENOMINATOR))
                .ok_or(CustomError::Overflow)?;

            // remove from collection owner price
            let owner_amount = total_price
                .checked_sub(royalty_amount)
                .ok_or(CustomError::Overflow)?;

            // Commission amount is added on top
            let commission_amount = if commission_bps > 0 {
                total_price
                    .checked_mul(commission_bps as u64)
                    .and_then(|v| v.checked_div(BPS_DENOMINATOR))
                    .ok_or(CustomError::Overflow)?
            } else {
                0
            };

            // validate
            let purchaser_token_amount = ctx.accounts.purchaser_token_account.amount;
            let total_to_pay = total_price
                .checked_add(commission_amount)
                .ok_or(CustomError::Overflow)?;
            require!(
                purchaser_token_amount >= total_to_pay,
                CustomError::InsufficientFunds
            );
            
            if royalty_amount > 0 {
                let cpi_accounts = Transfer {
                    from: ctx.accounts.purchaser_token_account.to_account_info(),
                    to: ctx.accounts.royalty_token_account.to_account_info(), // This must be passed in
                    authority: ctx.accounts.purchase_signer.to_account_info(),
                };
                let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
                token::transfer(cpi_ctx, royalty_amount)?;
            }
            
            if owner_amount > 0 {
                let cpi_accounts = Transfer {
                    from: ctx.accounts.purchaser_token_account.to_account_info(),
                    to: ctx.accounts.owner_token_account.to_account_info(),
                    authority: ctx.accounts.purchase_signer.to_account_info(),
                };
                let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
                token::transfer(cpi_ctx, owner_amount)?;
            }
            
            if commission_amount > 0 {
                let cpi_accounts = Transfer {
                    from: ctx.accounts.purchaser_token_account.to_account_info(),
                    to: ctx.accounts.commission_token_account.to_account_info(), // This must be passed in
                    authority: ctx.accounts.purchase_signer.to_account_info(),
                };
                let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
                token::transfer(cpi_ctx, commission_amount)?;
            }

        }


        if user_purchases.data.is_empty() {
            let bitmask_len = (collection.size as usize + 7) / 8;
            user_purchases.data = vec![0u8; bitmask_len];
        }
        for &i in &trait_indexes {
            user_purchases.set(i);
        }
    
        Ok(())
    }
}

#[derive(Accounts)]
pub struct UpdateCollectionPriceToken<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: PDA seed
    pub collection_address: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"prices", collection_address.key().as_ref()],
        bump = collection_prices_data.bump,
        has_one = owner @ CustomError::Unauthorized
    )]
    pub collection_prices_data: Account<'info, CollectionPricesData>,
}

// remove this function
#[derive(Accounts)]
pub struct UpdatePaymentMint<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: PDA seed
    pub collection_address: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"prices", collection_address.key().as_ref()],
        bump = collection_prices_data.bump,
        has_one = owner @ CustomError::Unauthorized
    )]
    pub collection_prices_data: Account<'info, CollectionPricesData>,
}

// remove this function
#[derive(Accounts)]
pub struct UpdatePrices<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: Used only for PDA derivation
    pub collection_address: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"prices", collection_address.key().as_ref()],
        bump = collection_prices_data.bump,
        has_one = owner @ CustomError::Unauthorized
    )]
    pub collection_prices_data: Account<'info, CollectionPricesData>,
}

#[derive(Accounts)]
pub struct GetRoyaltyPubkey {}



#[derive(Accounts)]
#[instruction(prices: Vec<u64>, payment_mint: Pubkey)]
pub struct InitializeCollection<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: Used only for PDA derivation
    pub collection_address: UncheckedAccount<'info>,

    #[account(init, payer = owner, space = 8 + CollectionPricesData::dynamic_size(prices.len()),
        seeds = [b"prices", collection_address.key().as_ref()], bump)]
    pub collection_prices_data: Account<'info, CollectionPricesData>,

    pub system_program: Program<'info, System>,
}

#[account]
pub struct CollectionPricesData {
    pub bump: u8,
    pub owner: Pubkey,         // Collection owner
    pub size: u16,             
    pub payment_mint: Pubkey,
    pub prices: Vec<u64>,
}

#[account]
pub struct UserPurchases {
    pub data: Vec<u8>, // bitmask
}

// calculate user purchases allocation size
impl UserPurchases {
    pub fn space(size: u16) -> usize {
        let byte_len = (size as usize + 7) / 8; // round up
        4 + byte_len // 4 for vec length prefix
    }

    pub fn has(&self, index: u16) -> bool {
        let byte = index as usize / 8;
        let bit = index % 8;
        if byte >= self.data.len() {
            return false;
        }
        self.data[byte] & (1 << bit) != 0
    }

    pub fn set(&mut self, index: u16) {
        let byte = index as usize / 8;
        let bit = index % 8;
        self.data[byte] |= 1 << bit;
    }
}

impl CollectionPricesData {
    pub fn dynamic_size(prices_len: usize) -> usize {
        // 4 + prices_len * 8 = Vec<u64> (4 bytes vec length + each u64 is 8 bytes)
        // bump, owner, size, payment mint, all prices size, prices values
        8 + 32 + 2 + 32 + 4 + prices_len * 8
    }
}


#[derive(Accounts)]
pub struct LamportsPurchase<'info> {
    #[account(mut)]
    pub purchaser: Signer<'info>,

    /// CHECK: Used for PDA derivation only
    pub collection_address: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"prices", collection_address.key().as_ref()],
        bump = collection_prices_data.bump
    )]
    pub collection_prices_data: Account<'info, CollectionPricesData>,

        /// Purchaser purchases PDA (init if needed)
    #[account(
        init_if_needed,
        payer = purchaser,
        space = 8 + 4 + (collection_prices_data.size as usize + 7) / 8,
        seeds = [b"purchases", collection_address.key().as_ref(), purchaser.key().as_ref()],
        bump
    )]
    pub user_purchases: Account<'info, UserPurchases>,

    #[account(mut)]
    pub owner: SystemAccount<'info>,

    #[account(mut)]
    pub app_royalty: SystemAccount<'info>,
    
    #[account(mut)]
    pub commission_wallet: SystemAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct TokenPurchase<'info> {
    #[account(mut)]
    pub purchase_signer: Signer<'info>,

    /// CHECK: Used for PDA derivation only
    pub collection_address: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"prices", collection_address.key().as_ref()],
        bump = collection_prices_data.bump
    )]
    pub collection_prices_data: Account<'info, CollectionPricesData>,

    /// Purchaser purchases PDA (init if needed)
    #[account(
        init_if_needed,
        payer = purchase_signer,
        space = 8 + 4 + (collection_prices_data.size as usize + 7) / 8,
        seeds = [b"purchases", collection_address.key().as_ref(), purchase_signer.key().as_ref()],
        bump
    )]
    pub user_purchases: Account<'info, UserPurchases>,

    // VALIDATED IT MATCHES IN collection_prices_data
    #[account(mut)]
    pub owner: SystemAccount<'info>,

    #[account(mut)]
    pub purchaser_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub owner_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub royalty_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub commission_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>,
}



#[error_code]
pub enum CustomError {
    #[msg("Unauthorized: only the owner can modify prices")]
    Unauthorized,
    #[msg("New prices array length must match existing")]
    PriceLengthMismatch,
    #[msg("Wallet has insufficient funds to create Collection.")]
    InsufficientFundsForRent,
    #[msg("Trait index is out of bounds.")]
    InvalidTraitIndex,
    #[msg("Overflow during price calculation.")]
    Overflow,
    #[msg("Empty price list sent as parameter.")]
    EmptyPriceList,
    #[msg("Missing Token Account.")]
    MissingTokenAccount,
    #[msg("Missing Token Program.")]
    MissingTokenProgram,
    #[msg("Invalid Purchase Mint Token.")]
    InvalidTokenMint,
    #[msg("Invalid Owner Mint Token.")]
    InvalidTokenOwner,
    #[msg("Expected Lamport Payment Call.")]
    ExpectedLamportsPayment,
    #[msg("Expected Token Payment Call.")]
    ExpectedTokenPayment,
    #[msg("No Traits Were Selected to purchase.")]
    NoTraitsSelected,
    #[msg("Invalid Collection Owner.")]
    InvalidOwner,
    #[msg("Purchase failed, Insufficient funds.")]
    InsufficientFunds,
    #[msg("Comission too high, Set to max 70%.")]
    CommissionTooHigh,
    #[msg("Invalid royalty account provided.")]
    InvalidRoyaltyAccount,
}
