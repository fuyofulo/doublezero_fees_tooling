use bytemuck::{Pod, Zeroable};
use ruint::aliases::U64;
use solana_program::pubkey::Pubkey;

pub const REVENUE_DISTRIBUTION_PROGRAM_ID: Pubkey = solana_program::pubkey!(
    "dzrevZC94tBLwuHw1dyynZxaXTWyp7yocsinyEVPtt4"
);

pub type Flags = U64;
pub type EpochDuration = u32;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Pod, Zeroable)]
#[repr(C)]
pub struct DoubleZeroEpoch(pub u64);

impl DoubleZeroEpoch {
    pub fn value(&self) -> u64 {
        self.0
    }

    pub fn as_seed(&self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    pub fn checked_sub_duration(&self, duration: EpochDuration) -> Option<Self> {
        let value = self.0.checked_sub(duration as u64)?;
        Some(Self(value))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Pod, Zeroable)]
#[repr(C)]
pub struct UnitShare16(pub u16);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Pod, Zeroable)]
#[repr(C)]
pub struct UnitShare32(pub u32);

impl UnitShare32 {
    pub const MAX: u64 = 1_000_000_000;
}

pub type ValidatorFee = UnitShare16;
pub type BurnRate = UnitShare32;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C, align(8))]
pub struct SolanaValidatorFeeParameters {
    pub base_block_rewards_pct: ValidatorFee,
    pub priority_block_rewards_pct: ValidatorFee,
    pub inflation_rewards_pct: ValidatorFee,
    pub jito_tips_pct: ValidatorFee,
    pub fixed_sol_amount: u32,
    _storage_gap: [u32; 7],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C, align(8))]
pub struct CommunityBurnRateParameters {
    pub limit: BurnRate,
    pub dz_epochs_to_increasing: EpochDuration,
    pub dz_epochs_to_limit: EpochDuration,
    cached_slope_numerator: BurnRate,
    cached_slope_denominator: EpochDuration,
    cached_next_burn_rate: BurnRate,
}

impl CommunityBurnRateParameters {
    pub fn next_burn_rate_raw(&self) -> Option<u64> {
        if self.cached_next_burn_rate.0 == 0 {
            None
        } else {
            Some(self.cached_next_burn_rate.0 as u64)
        }
    }

    pub fn mode(&self) -> &'static str {
        if self.dz_epochs_to_increasing != 0 {
            "Static"
        } else if self.dz_epochs_to_limit != 0 {
            "Increasing"
        } else {
            "Limit"
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C, align(8))]
pub struct DistributionParameters {
    pub calculation_grace_period_minutes: u16,
    pub initialization_grace_period_minutes: u16,
    pub minimum_epoch_duration_to_finalize_rewards: u8,
    _padding: [u8; 3],
    pub community_burn_rate_parameters: CommunityBurnRateParameters,
    pub solana_validator_fee_parameters: SolanaValidatorFeeParameters,
    _storage_gap: StorageGap<8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C, align(8))]
pub struct RelayParameters {
    pub placeholder_lamports: u32,
    pub distribute_rewards_lamports: u32,
    _storage_gap: StorageGap<1>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C, align(8))]
pub struct ProgramConfig {
    pub flags: Flags,
    pub next_completed_dz_epoch: DoubleZeroEpoch,
    pub bump_seed: u8,
    pub reserve_2z_bump_seed: u8,
    pub swap_authority_bump_seed: u8,
    pub swap_destination_2z_bump_seed: u8,
    pub withdraw_sol_authority_bump_seed: u8,
    _padding_0: [u8; 3],
    pub admin_key: Pubkey,
    pub debt_accountant_key: Pubkey,
    pub rewards_accountant_key: Pubkey,
    pub contributor_manager_key: Pubkey,
    pub placeholder_key: Pubkey,
    pub sol_2z_swap_program_id: Pubkey,
    pub distribution_parameters: DistributionParameters,
    pub relay_parameters: RelayParameters,
    pub last_initialized_distribution_timestamp: u32,
    _padding_1: [u8; 4],
    pub debt_write_off_feature_activation_epoch: DoubleZeroEpoch,
}

impl ProgramConfig {
    pub const SEED_PREFIX: &'static [u8] = b"program_config";

    pub fn find_address() -> (Pubkey, u8) {
        Pubkey::find_program_address(&[Self::SEED_PREFIX], &REVENUE_DISTRIBUTION_PROGRAM_ID)
    }

    pub fn is_paused(&self) -> bool {
        flag_is_set(self.flags, 0)
    }

    pub fn is_migrated(&self) -> bool {
        flag_is_set(self.flags, 1)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C, align(8))]
pub struct Distribution {
    pub dz_epoch: DoubleZeroEpoch,
    pub flags: Flags,
    pub community_burn_rate: BurnRate,
    pub bump_seed: u8,
    pub token_2z_pda_bump_seed: u8,
    _padding_0: [u8; 2],
    pub solana_validator_fee_parameters: SolanaValidatorFeeParameters,
    pub solana_validator_debt_merkle_root: [u8; 32],
    pub total_solana_validators: u32,
    pub solana_validator_payments_count: u32,
    pub total_solana_validator_debt: u64,
    pub collected_solana_validator_payments: u64,
    pub rewards_merkle_root: [u8; 32],
    pub total_contributors: u32,
    pub distributed_rewards_count: u32,
    pub collected_prepaid_2z_payments: u64,
    pub collected_2z_converted_from_sol: u64,
    pub uncollectible_sol_debt: u64,
    pub processed_solana_validator_debt_start_index: u32,
    pub processed_solana_validator_debt_end_index: u32,
    pub processed_rewards_start_index: u32,
    pub processed_rewards_end_index: u32,
    pub distribute_rewards_relay_lamports: u32,
    pub calculation_allowed_timestamp: u32,
    pub distributed_2z_amount: u64,
    pub burned_2z_amount: u64,
    pub processed_solana_validator_debt_write_off_start_index: u32,
    pub processed_solana_validator_debt_write_off_end_index: u32,
    pub solana_validator_write_off_count: u32,
    _padding_1: [u8; 20],
    _storage_gap: StorageGap<6>,
}

impl Distribution {
    pub const SEED_PREFIX: &'static [u8] = b"distribution";

    pub fn find_address(dz_epoch: DoubleZeroEpoch) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[Self::SEED_PREFIX, &dz_epoch.as_seed()],
            &REVENUE_DISTRIBUTION_PROGRAM_ID,
        )
    }

    pub fn is_debt_calculation_finalized(&self) -> bool {
        flag_is_set(self.flags, 1)
    }

    pub fn is_rewards_calculation_finalized(&self) -> bool {
        flag_is_set(self.flags, 2)
    }

    pub fn has_swept_2z_tokens(&self) -> bool {
        flag_is_set(self.flags, 3)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C, align(8))]
pub struct U128Le(pub [u64; 2]);

impl U128Le {
    pub fn to_u128(&self) -> u128 {
        (self.0[0] as u128) | ((self.0[1] as u128) << 64)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C, align(8))]
pub struct Journal {
    pub bump_seed: u8,
    pub token_2z_pda_bump_seed: u8,
    _padding: [u8; 6],
    pub total_sol_balance: u64,
    pub total_2z_balance: u64,
    pub swap_2z_destination_balance: u64,
    pub swapped_sol_amount: u64,
    pub next_dz_epoch_to_sweep_tokens: DoubleZeroEpoch,
    pub lifetime_swapped_2z_amount: U128Le,
}

impl Journal {
    pub const SEED_PREFIX: &'static [u8] = b"journal";

    pub fn find_address() -> (Pubkey, u8) {
        Pubkey::find_program_address(&[Self::SEED_PREFIX], &REVENUE_DISTRIBUTION_PROGRAM_ID)
    }

    pub fn lifetime_swapped_2z_amount(&self) -> u128 {
        self.lifetime_swapped_2z_amount.to_u128()
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageGap<const N: usize>(pub [[u8; 32]; N]);

impl<const N: usize> Default for StorageGap<N> {
    fn default() -> Self {
        Self([Default::default(); N])
    }
}

unsafe impl<const N: usize> Zeroable for StorageGap<N> {}
unsafe impl<const N: usize> Pod for StorageGap<N> {}

fn flag_is_set(flags: Flags, bit: usize) -> bool {
    let raw: u64 = flags.try_into().unwrap_or_default();
    ((raw >> bit) & 1) == 1
}
