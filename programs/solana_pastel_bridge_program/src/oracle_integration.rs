use anchor_lang::prelude::*;
use anchor_lang::solana_program::entrypoint::ProgramResult;
use anchor_lang::solana_program::account_info::AccountInfo;
use anchor_lang::solana_program::sysvar::clock::Clock;
use anchor_lang::solana_program::hash::{hash, Hash};

const REGISTRATION_ENTRANCE_FEE_IN_LAMPORTS: u64 = 10_000_000; // 0.10 SOL in lamports
const MIN_NUMBER_OF_ORACLES: usize = 8; // Minimum number of oracles to calculate consensus
const MIN_REPORTS_FOR_REWARD: u32 = 10; // Data Contributor must submit at least 10 reports to be eligible for rewards
const MIN_COMPLIANCE_SCORE_FOR_REWARD: f32 = 65.0; // Data Contributor must have a compliance score of at least 80 to be eligible for rewards
const MIN_RELIABILITY_SCORE_FOR_REWARD: f32 = 80.0; // Minimum reliability score to be eligible for rewards
const BASE_REWARD_AMOUNT_IN_LAMPORTS: u64 = 100_000; // 0.0001 SOL in lamports is the base reward amount, which is scaled based on the number of highly reliable contributors
const COST_IN_LAMPORTS_OF_ADDING_PASTEL_TXID_FOR_MONITORING: u64 = 100_000; // 0.0001 SOL in lamports
const PERMANENT_BAN_THRESHOLD: u32 = 100; // Number of non-consensus report submissions for permanent ban
const CONTRIBUTIONS_FOR_PERMANENT_BAN: u32 = 250; // Considered for permanent ban after 250 contributions
const TEMPORARY_BAN_THRESHOLD: u32 = 5; // Number of non-consensus report submissions for temporary ban
const CONTRIBUTIONS_FOR_TEMPORARY_BAN: u32 = 50; // Considered for temporary ban after 50 contributions
const TEMPORARY_BAN_DURATION: u64 =  24 * 60 * 60; // Duration of temporary ban in seconds (e.g., 1 day)
const MAX_DURATION_IN_SECONDS_FROM_LAST_REPORT_SUBMISSION_BEFORE_COMPUTING_CONSENSUS: u64 = 10 * 60; // Maximum duration in seconds from last report submission for a given TXID before computing consensus (e.g., 10 minutes)
const DATA_RETENTION_PERIOD: u64 = 24 * 60 * 60; // How long to keep data in the contract state (1 day)
const SUBMISSION_COUNT_RETENTION_PERIOD: u64 = 24 * 60 * 60; // Number of seconds to retain submission counts (i.e., 24 hours)
const TXID_STATUS_VARIANT_COUNT: usize = 4; // Manually define the number of variants in TxidStatus
const MAX_TXID_LENGTH: usize = 64; // Maximum length of a TXID


#[error_code]
pub enum OracleError {
    ContributorAlreadyRegistered,
    UnregisteredOracle,
    InvalidTxid,
    InvalidFileHashLength,
    MissingPastelTicketType,
    MissingFileHash,
    RegistrationFeeNotPaid,
    NotEligibleForReward,
    NotBridgeContractAddress,
    InsufficientFunds,
    UnauthorizedWithdrawalAccount,
    InvalidPaymentAmount,
    PaymentNotFound,
    PendingPaymentAlreadyInitialized,
    AccountAlreadyInitialized,
    PendingPaymentInvalidAmount,
    InvalidPaymentStatus,
    InvalidTxidStatus,
    InvalidPastelTicketType,
    ContributorNotRegistered,
    ContributorBanned,
    EnoughReportsSubmittedForTxid
}

impl From<OracleError> for ProgramError {
    fn from(e: OracleError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

pub fn create_seed(seed_preamble: &str, txid: &str, reward_address: &Pubkey) -> Hash {
    // Concatenate the string representations. Reward address is Base58-encoded by default.
    let preimage_string = format!("{}{}{}", seed_preamble, txid, reward_address);
    // msg!("create_seed: generated preimage string: {}", preimage_string);
    // Convert the concatenated string to bytes
    let preimage_bytes = preimage_string.as_bytes();
    // Compute hash
    let seed_hash = hash(preimage_bytes);
    // msg!("create_seed: generated seed hash: {:?}", seed_hash);
    seed_hash
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, AnchorSerialize, AnchorDeserialize)]
pub enum TxidStatus {
    Invalid,
    PendingMining,
    MinedPendingActivation,
    MinedActivated,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, AnchorSerialize, AnchorDeserialize)]
pub enum PastelTicketType {
    Sense,
    Cascade,
    Nft,
    InferenceApi,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub struct PastelTxStatusReport {
    pub txid: String,
    pub txid_status: TxidStatus,
    pub pastel_ticket_type: Option<PastelTicketType>,
    pub first_6_characters_of_sha3_256_hash_of_corresponding_file: Option<String>,
    pub timestamp: u64,
    pub contributor_reward_address: Pubkey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub struct CommonReportData {
    pub txid: String,
    pub txid_status: TxidStatus,
    pub pastel_ticket_type: Option<PastelTicketType>,
    pub first_6_characters_of_sha3_256_hash_of_corresponding_file: Option<String>,
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct SpecificReportData {
    pub contributor_reward_address: Pubkey,
    pub timestamp: u64,
    pub common_data_ref: u64, // Reference to CommonReportData
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct TempTxStatusReport {
    pub common_data_ref: u64, // Index to CommonReportData in common_reports
    pub specific_data: SpecificReportData,
}

#[account]
pub struct TempTxStatusReportAccount {
    pub reports: Vec<TempTxStatusReport>,
    pub common_reports: Vec<CommonReportData>,
    pub specific_reports: Vec<SpecificReportData>,    
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct TxidSubmissionCount {
    pub txid: String,
    pub count: u32,
    pub last_updated: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct PendingPayment {
    pub txid: String,
    pub expected_amount: u64,
    pub payment_status: PaymentStatus,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub enum PaymentStatus {
    Pending,
    Received
}

#[account]
pub struct RewardPool {
    // Since this account is only used for holding and transferring SOL, no fields are necessary.
}

#[account]
pub struct FeeReceivingContract {
    // Since this account is only used for holding and transferring SOL, no fields are necessary.
}

#[account]
pub struct PastelTxStatusReportAccount {
    pub report: PastelTxStatusReport,
}

#[derive(Accounts)]
#[instruction(txid: String, reward_address: Pubkey)]
pub struct SubmitDataReport<'info> {
    #[account(
        init_if_needed,
        payer = user,
        seeds = [create_seed("pastel_tx_status_report", &txid, &user.key()).as_ref()],
        bump,
        space = 8 + (64 + 1 + 2 + 7 + 8 + 32 + 128) // Discriminator +  txid String (max length of 64) + txid_status + pastel_ticket_type + first_6_characters_of_sha3_256_hash_of_corresponding_file + timestamp + contributor_reward_address + cushion
    )]
    pub report_account: Account<'info, PastelTxStatusReportAccount>,

    #[account(mut)]
    pub oracle_contract_state: Account<'info, OracleContractState>,

    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(mut, seeds = [b"temp_tx_status_report"], bump)]
    pub temp_report_account: Account<'info, TempTxStatusReportAccount>,

    #[account(mut, seeds = [b"contributor_data"], bump)]
    pub contributor_data_account: Account<'info, ContributorDataAccount>,

    #[account(mut, seeds = [b"txid_submission_counts"], bump)]
    pub txid_submission_counts_account: Account<'info, TxidSubmissionCountsAccount>,

    #[account(mut, seeds = [b"aggregated_consensus_data"], bump)]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,

    pub system_program: Program<'info, System>,
}

fn update_submission_count(
    txid_submission_counts_account: &mut Account<TxidSubmissionCountsAccount>, 
    txid: &str
) -> Result<()> {
    // Get the current timestamp
    let current_timestamp_u64 = Clock::get()?.unix_timestamp as u64;

    // Check if the txid already exists in the submission counts
    if let Some(count) = txid_submission_counts_account.submission_counts.iter_mut().find(|c| c.txid == txid) {
        // Update the existing count
        count.count += 1;
        count.last_updated = current_timestamp_u64;
    } else {
        // Insert a new count if the txid does not exist
        txid_submission_counts_account.submission_counts.push(TxidSubmissionCount {
            txid: txid.to_string(),
            count: 1,
            last_updated: current_timestamp_u64,
        });
    }

    Ok(())
}

pub fn get_report_account_pda(
    program_id: &Pubkey, 
    txid: &str, 
    contributor_reward_address: &Pubkey
) -> (Pubkey, u8) {
    msg!("get_report_account_pda: program_id: {}, txid: {}, contributor_reward_address: {}", program_id, txid, contributor_reward_address);
    let seed_hash = create_seed("pastel_tx_status_report", txid, contributor_reward_address);
    msg!("get_report_account_pda: seed_hash: {:?}", seed_hash);
    Pubkey::find_program_address(&[seed_hash.as_ref()], program_id)
}


fn get_aggregated_data<'a>(
    aggregated_data_account: &'a Account<AggregatedConsensusDataAccount>,
    txid: &str
) -> Option<&'a AggregatedConsensusData> {
    aggregated_data_account.consensus_data.iter()
        .find(|data| data.txid == txid)
}


fn compute_consensus(aggregated_data: &AggregatedConsensusData) -> (TxidStatus, String) {
    let consensus_status = aggregated_data.status_weights.iter().enumerate().max_by_key(|&(_, weight)| weight)
        .map(|(index, _)| usize_to_txid_status(index).unwrap_or(TxidStatus::Invalid)).unwrap();

    let consensus_hash = aggregated_data.hash_weights.iter().max_by_key(|hash_weight| hash_weight.weight)
        .map(|hash_weight| hash_weight.hash.clone()).unwrap_or_default();

    (consensus_status, consensus_hash)
}


fn apply_bans(contributor: &mut Contributor, current_timestamp: u64, is_accurate: bool) {
    if !is_accurate {

        if contributor.total_reports_submitted <= CONTRIBUTIONS_FOR_TEMPORARY_BAN && contributor.consensus_failures % TEMPORARY_BAN_THRESHOLD == 0 {
            contributor.ban_expiry = current_timestamp + TEMPORARY_BAN_DURATION;
            msg!("Contributor: {} is temporarily banned as of {} because they have submitted {} reports and have {} consensus failures, more than the maximum allowed consensus failures of {}. Ban expires on: {}", 
            contributor.reward_address, current_timestamp, contributor.total_reports_submitted, contributor.consensus_failures, TEMPORARY_BAN_THRESHOLD, contributor.ban_expiry);
        } else if contributor.total_reports_submitted >= CONTRIBUTIONS_FOR_PERMANENT_BAN && contributor.consensus_failures >= PERMANENT_BAN_THRESHOLD {
            contributor.ban_expiry = u64::MAX;
            msg!("Contributor: {} is permanently banned as of {} because they have submitted {} reports and have {} consensus failures, more than the maximum allowed consensus failures of {}. Removing from list of contributors!", 
            contributor.reward_address, current_timestamp, contributor.total_reports_submitted, contributor.consensus_failures, PERMANENT_BAN_THRESHOLD);
        }
    }
}

fn update_scores(contributor: &mut Contributor, current_timestamp: u64, is_accurate: bool) {
    let time_diff = current_timestamp.saturating_sub(contributor.last_active_timestamp);
    let hours_inactive: f32 = time_diff as f32 / 3_600.0;

    // Dynamic scaling for accuracy
    let accuracy_scaling = if is_accurate {
        (1.0 + contributor.current_streak as f32 * 0.1).min(2.0) // Increasing bonus for consecutive accuracy
    } else {
        1.0
    };

    let time_weight = 1.0 / (1.0 + hours_inactive / 480.0);

    let base_score_increment = 20.0; // Adjusted base increment for a more gradual increase

    let score_increment = base_score_increment * accuracy_scaling * time_weight;

    // Exponential penalty for inaccuracies
    let score_decrement = 20.0 * (1.0 + contributor.consensus_failures as f32 * 0.5).min(3.0); 

    let decay_rate: f32 = 0.99; // Adjusted decay rate
    let decay_factor = decay_rate.powf(hours_inactive / 24.0);

    let streak_bonus = if is_accurate {
        (contributor.current_streak as f32 / 10.0).min(3.0).max(0.0) // Enhanced streak bonus
    } else {
        0.0
    };

    if is_accurate {
        contributor.total_reports_submitted += 1;
        contributor.accurate_reports_count += 1;
        contributor.current_streak += 1;
        contributor.compliance_score += score_increment + streak_bonus;
    } else {
        contributor.total_reports_submitted += 1;
        contributor.current_streak = 0;
        contributor.consensus_failures += 1;
        contributor.compliance_score = (contributor.compliance_score - score_decrement).max(0.0);
    }

    contributor.compliance_score *= decay_factor;

    // Integrating reliability score into compliance score calculation
    let reliability_factor = (contributor.accurate_reports_count as f32 / contributor.total_reports_submitted as f32).clamp(0.0, 1.0);
    contributor.compliance_score = (contributor.compliance_score * reliability_factor).min(100.0);

    contributor.compliance_score = logistic_scale(contributor.compliance_score, 100.0, 0.1, 50.0); // Adjusted logistic scaling

    contributor.reliability_score = reliability_factor * 100.0;

    log_score_updates(contributor);
}

fn logistic_scale(score: f32, max_value: f32, steepness: f32, midpoint: f32) -> f32 {
    max_value / (1.0 + (-steepness * (score - midpoint)).exp())
}

fn log_score_updates(contributor: &Contributor) {
    msg!("Scores After Update: Address: {}, Compliance Score: {}, Reliability Score: {}",
        contributor.reward_address, contributor.compliance_score, contributor.reliability_score);
}

fn update_statuses(contributor: &mut Contributor, current_timestamp: u64) {
    // Updating recently active status
    let recent_activity_threshold = 86_400; // 24 hours in seconds
    contributor.is_recently_active = current_timestamp - contributor.last_active_timestamp < recent_activity_threshold;

    // Updating reliability status
    contributor.is_reliable = if contributor.total_reports_submitted > 0 {
        let reliability_ratio = contributor.accurate_reports_count as f32 / contributor.total_reports_submitted as f32;
        reliability_ratio >= 0.8 // Example threshold for reliability
    } else {
        false
    };

    // Updating eligibility for rewards
    contributor.is_eligible_for_rewards = contributor.total_reports_submitted >= MIN_REPORTS_FOR_REWARD 
        && contributor.reliability_score >= MIN_RELIABILITY_SCORE_FOR_REWARD 
        && contributor.compliance_score >= MIN_COMPLIANCE_SCORE_FOR_REWARD;
}

fn update_contributor(contributor: &mut Contributor, current_timestamp: u64, is_accurate: bool) {
    // Check if the contributor is banned before proceeding. If so, just return.
    if contributor.calculate_is_banned(current_timestamp) {
        msg!("Contributor is currently banned and cannot be updated: {}", contributor.reward_address);
        return; // We don't stop the process here, just skip this contributor.
    }

    // Updating scores
    update_scores(contributor, current_timestamp, is_accurate);

    // Applying bans based on report accuracy
    apply_bans(contributor, current_timestamp, is_accurate);

    // Updating contributor statuses
    update_statuses(contributor, current_timestamp);
}


fn calculate_consensus(
    aggregated_data_account: &Account<AggregatedConsensusDataAccount>,    
    temp_report_account: &TempTxStatusReportAccount,
    contributor_data_account: &mut Account<ContributorDataAccount>,
    txid: &str,
) -> Result<()> {
    let current_timestamp = Clock::get()?.unix_timestamp as u64;
    let (consensus_status, consensus_hash) = get_aggregated_data(aggregated_data_account, txid)
        .map(|data| compute_consensus(data))
        .unwrap_or((TxidStatus::Invalid, String::new()));

    let mut updated_contributors = Vec::new();
    let mut contributor_count = 0;

    for temp_report in temp_report_account.reports.iter() {
        let common_data = &temp_report_account.common_reports[temp_report.common_data_ref as usize];
        let specific_data = &temp_report.specific_data;
    
        if common_data.txid == txid && !updated_contributors.contains(&specific_data.contributor_reward_address) {
            if let Some(contributor) = contributor_data_account.contributors.iter_mut().find(|c| c.reward_address == specific_data.contributor_reward_address) {
                let is_accurate = common_data.txid_status == consensus_status &&
                    common_data.first_6_characters_of_sha3_256_hash_of_corresponding_file.as_ref().map_or(false, |hash| hash == &consensus_hash);
                update_contributor(contributor, current_timestamp, is_accurate);
                updated_contributors.push(specific_data.contributor_reward_address);
            }
            contributor_count += 1;
        }
    }
    msg!("Consensus reached for TXID: {}, Status: {:?}, Hash: {}, Number of Contributors Included: {}", txid, consensus_status, consensus_hash, contributor_count);

    Ok(())
}


pub fn apply_permanent_bans(contributor_data_account: &mut Account<ContributorDataAccount>) {
    // Collect addresses of contributors to be removed for efficient logging
    let contributors_to_remove: Vec<String> = contributor_data_account.contributors.iter()
        .filter(|c| c.ban_expiry == u64::MAX)
        .map(|c| c.reward_address.to_string()) // Convert Pubkey to String
        .collect();

    // Log information about the removal process
    msg!("Now removing permanently banned contributors! Total number of contributors before removal: {}, Number of contributors to be removed: {}, Addresses of contributors to be removed: {:?}",
        contributor_data_account.contributors.len(), contributors_to_remove.len(), contributors_to_remove);

    // Retain only contributors who are not permanently banned
    contributor_data_account.contributors.retain(|c| c.ban_expiry != u64::MAX);
}


fn post_consensus_tasks(
    txid_submission_counts_account: &mut Account<TxidSubmissionCountsAccount>,    
    aggregated_data_account: &mut Account<AggregatedConsensusDataAccount>,
    temp_report_account: &mut TempTxStatusReportAccount,
    contributor_data_account: &mut Account<ContributorDataAccount>,
    txid: &str,
) -> Result<()> {
    let current_timestamp = Clock::get()?.unix_timestamp as u64;

    apply_permanent_bans(contributor_data_account);

    msg!("Now cleaning up unneeded data in TempTxStatusReportAccount...");
    // Cleanup unneeded data in TempTxStatusReportAccount
    temp_report_account.reports.retain(|temp_report| {
        // Access the common data from the TempTxStatusReportAccount
        let common_data = &temp_report_account.common_reports[temp_report.common_data_ref as usize];
        let specific_data = &temp_report.specific_data;
        common_data.txid != txid && current_timestamp - specific_data.timestamp < DATA_RETENTION_PERIOD
    });

    msg!("Now cleaning up unneeded data in AggregatedConsensusDataAccount...");
    // Cleanup unneeded data in AggregatedConsensusDataAccount
    aggregated_data_account.consensus_data.retain(|data| {
        current_timestamp - data.last_updated < DATA_RETENTION_PERIOD
    });

    msg!("Now cleaning up unneeded data in TxidSubmissionCountsAccount...");
    // Cleanup old submission counts in TxidSubmissionCountsAccount
    txid_submission_counts_account.submission_counts.retain(|count| {
        current_timestamp - count.last_updated < SUBMISSION_COUNT_RETENTION_PERIOD
    });

    msg!("Done with post-consensus tasks!");
    Ok(())
}


fn aggregate_consensus_data(
    aggregated_data_account: &mut Account<AggregatedConsensusDataAccount>, 
    report: &PastelTxStatusReport, 
    weight: f32, 
    txid: &str
) -> Result<()> {
    let scaled_weight = (weight * 100.0) as i32; // Scaling by a factor of 100
    let current_timestamp = Clock::get()?.unix_timestamp as u64;

    // Check if the txid already exists in the aggregated consensus data
    if let Some(data_entry) = aggregated_data_account.consensus_data.iter_mut().find(|d| d.txid == txid) {
        // Update existing data
        data_entry.status_weights[report.txid_status as usize] += scaled_weight;
        if let Some(hash) = &report.first_6_characters_of_sha3_256_hash_of_corresponding_file {
            update_hash_weight(&mut data_entry.hash_weights, hash, scaled_weight);
        }
        data_entry.last_updated = current_timestamp;
        // Handling the Option<String> here
        data_entry.first_6_characters_of_sha3_256_hash_of_corresponding_file = 
            report.first_6_characters_of_sha3_256_hash_of_corresponding_file.clone().unwrap_or_default();
    } else {
        // Create new data
        let mut new_data = AggregatedConsensusData {
            txid: txid.to_string(),
            status_weights: [0; TXID_STATUS_VARIANT_COUNT],
            hash_weights: Vec::new(),
            first_6_characters_of_sha3_256_hash_of_corresponding_file: 
                report.first_6_characters_of_sha3_256_hash_of_corresponding_file.clone().unwrap_or_default(),
            last_updated: current_timestamp,
        };
        new_data.status_weights[report.txid_status as usize] += scaled_weight;
        if let Some(hash) = &report.first_6_characters_of_sha3_256_hash_of_corresponding_file {
            new_data.hash_weights.push(HashWeight { hash: hash.clone(), weight: scaled_weight });
        }
        aggregated_data_account.consensus_data.push(new_data);
    }

    Ok(())
}


fn find_or_add_common_report_data(
    temp_report_account: &mut TempTxStatusReportAccount, 
    common_data: &CommonReportData
) -> u64 {
    if let Some((index, _)) = temp_report_account.common_reports.iter().enumerate().find(|(_, data)| *data == common_data) {
        index as u64
    } else {
        temp_report_account.common_reports.push(common_data.clone());
        (temp_report_account.common_reports.len() - 1) as u64
    }
}


pub fn submit_data_report_helper(
    ctx: Context<SubmitDataReport>, 
    txid: String, 
    report: PastelTxStatusReport,
    contributor_reward_address: Pubkey
) -> ProgramResult {
    // Directly access accounts from the context
    let txid_submission_counts_account: &mut Account<'_, TxidSubmissionCountsAccount> = &mut ctx.accounts.txid_submission_counts_account;
    let aggregated_data_account = &mut ctx.accounts.aggregated_consensus_data_account;
    let temp_report_account = &mut ctx.accounts.temp_report_account;
    let contributor_data_account = &mut ctx.accounts.contributor_data_account;


    // Retrieve the submission count for the given txid from the PDA account
    let txid_submission_count: usize = txid_submission_counts_account.submission_counts.iter()
        .find(|c| c.txid == txid).map_or(0, |c| c.count as usize);

    // Check if the number of submissions is already at or exceeds MIN_NUMBER_OF_ORACLES
    if txid_submission_count >= MIN_NUMBER_OF_ORACLES {
        msg!("Enough reports have already been submitted for this txid");
        return Err(OracleError::EnoughReportsSubmittedForTxid.into());
    }    

    // Validate the data report before any contributor-specific checks
    // msg!("Validating data report: {:?}", report);
    validate_data_contributor_report(&report)?;

    // Check if the contributor is registered and not banned
    // msg!("Checking if contributor is registered and not banned");    
    let contributor = contributor_data_account.contributors
        .iter()
        .find(|c| c.reward_address == contributor_reward_address)
        .ok_or(OracleError::ContributorNotRegistered)?;

    if contributor.calculate_is_banned(Clock::get()?.unix_timestamp as u64) {
        return Err(OracleError::ContributorBanned.into());
    }

    // Clone the String before using it
    let first_6_characters_of_sha3_256_hash_of_corresponding_file = report.first_6_characters_of_sha3_256_hash_of_corresponding_file.clone();

    // Extracting common data from the report
    // msg!("Extracting common data from the report");
    let common_data = CommonReportData {
        txid: report.txid.clone(),
        txid_status: report.txid_status,
        pastel_ticket_type: report.pastel_ticket_type,
        first_6_characters_of_sha3_256_hash_of_corresponding_file: first_6_characters_of_sha3_256_hash_of_corresponding_file,
    };

    // Finding or adding common report data
    // msg!("Finding or adding common report data");
    let common_data_index = find_or_add_common_report_data(temp_report_account, &common_data);

    // Creating specific report data
    // msg!("Creating specific report data");
    let specific_report = SpecificReportData {
        contributor_reward_address,
        timestamp: report.timestamp,
        common_data_ref: common_data_index,
    };

    // Creating a temporary report entry
    // msg!("Creating a temporary report entry");
    let temp_report: TempTxStatusReport = TempTxStatusReport {
        common_data_ref: common_data_index,
        specific_data: specific_report,
    };

    // Add the temporary report to the TempTxStatusReportAccount
    // msg!("Adding the temporary report to the TempTxStatusReportAccount");
    temp_report_account.reports.push(temp_report);

    // Update submission count and consensus-related data
    // msg!("Updating submission count and consensus-related data");
    update_submission_count(txid_submission_counts_account, &txid)?;

    let compliance_score = contributor.compliance_score;
    let reliability_score = contributor.reliability_score;
    let weight: f32 = compliance_score + reliability_score;
    aggregate_consensus_data(aggregated_data_account, &report, weight, &txid)?;
    
    // Check for consensus and perform related tasks
    if should_calculate_consensus(txid_submission_counts_account, &txid)? {

        msg!("We now have enough reports to calculate consensus for txid: {}", txid);
        
        let contributor_data_account: &mut Account<'_, ContributorDataAccount> = &mut ctx.accounts.contributor_data_account;
        msg!("Calculating consensus...");
        calculate_consensus(aggregated_data_account, temp_report_account, contributor_data_account, &txid)?;

        msg!("Performing post-consensus tasks...");
        post_consensus_tasks(txid_submission_counts_account, aggregated_data_account, temp_report_account, contributor_data_account, &txid)?;
    }

    // Log the new size of temp_tx_status_reports
    msg!("New size of temp_tx_status_reports in bytes after processing report for txid {} from contributor {}: {}", txid, contributor_reward_address, temp_report_account.reports.len() * std::mem::size_of::<TempTxStatusReport>());

    Ok(())
}


#[derive(Accounts)]
#[instruction(txid: String)]
pub struct HandleConsensus<'info> {

    #[account(mut)]
    pub oracle_contract_state: Account<'info, OracleContractState>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[account]
pub struct PendingPaymentAccount {
    pub pending_payment: PendingPayment,
}

#[derive(Accounts)]
#[instruction(txid: String)]
pub struct HandlePendingPayment<'info> {
    #[account(
        init_if_needed,
        payer = user,
        seeds = [create_seed("pending_payment", &txid, &user.key()).as_ref()],
        bump,
        space = 8 + std::mem::size_of::<PendingPayment>() + 64 // Adjusted for discriminator
    )]
    pub pending_payment_account: Account<'info, PendingPaymentAccount>,

    #[account(mut)]
    pub oracle_contract_state: Account<'info, OracleContractState>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn add_pending_payment_helper(
    ctx: Context<HandlePendingPayment>, 
    txid: String, 
    pending_payment: PendingPayment
) -> ProgramResult {
    let pending_payment_account = &mut ctx.accounts.pending_payment_account;

    // Ensure the account is being initialized for the first time to avoid re-initialization
    if !pending_payment_account.pending_payment.txid.is_empty() && pending_payment_account.pending_payment.txid != txid {
        return Err(OracleError::PendingPaymentAlreadyInitialized.into());
    }

    // Ensure txid is correct and other fields are properly set
    if pending_payment.txid != txid {
        return Err(OracleError::InvalidTxid.into());
    }

    // Store the pending payment in the account
    pending_payment_account.pending_payment = pending_payment;

    msg!("Pending payment account initialized: TXID: {}, Expected Amount: {}, Status: {:?}", 
        pending_payment_account.pending_payment.txid, 
        pending_payment_account.pending_payment.expected_amount, 
        pending_payment_account.pending_payment.payment_status);

    Ok(())
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct Contributor {
    pub reward_address: Pubkey,
    pub registration_entrance_fee_transaction_signature: String,
    pub compliance_score: f32,
    pub last_active_timestamp: u64,
    pub total_reports_submitted: u32,
    pub accurate_reports_count: u32,
    pub current_streak: u32,
    pub reliability_score: f32,
    pub consensus_failures: u32,
    pub ban_expiry: u64,
    pub is_eligible_for_rewards: bool,
    pub is_recently_active: bool,
    pub is_reliable: bool,    
}

#[account]
pub struct ContributorDataAccount {
    pub contributors: Vec<Contributor>,
}

#[account]
pub struct TxidSubmissionCountsAccount {
    pub submission_counts: Vec<TxidSubmissionCount>,
}

#[account]
pub struct AggregatedConsensusDataAccount {
    pub consensus_data: Vec<AggregatedConsensusData>,
}


#[account]
pub struct OracleContractState {
    pub is_initialized: bool,
    pub admin_pubkey: Pubkey,
    pub txid_submission_counts: Vec<TxidSubmissionCount>,
    pub monitored_txids: Vec<String>,
    pub reward_pool_account: Pubkey,
    pub fee_receiving_contract_account: Pubkey,
    pub txid_submission_counts_account: Pubkey,    
    pub aggregated_consensus_data_account: Pubkey,    
    pub bridge_contract_pubkey: Pubkey,
}


#[derive(Accounts)]
#[instruction(admin_pubkey: Pubkey)]
pub struct Initialize<'info> {
    #[account(init, payer = user, space = 10_240)] // Adjusted space
    pub oracle_contract_state: Account<'info, OracleContractState>,
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        init,
        seeds = [b"reward_pool"],
        bump,
        payer = user,
        space = 8 + 1024 // Adjust the space as needed
    )]
    pub reward_pool_account: Account<'info, RewardPool>,

    #[account(
        init,
        seeds = [b"fee_receiving_contract"],
        bump,
        payer = user,
        space = 8 + 1024 // Adjust the space as needed
    )]
    pub fee_receiving_contract_account: Account<'info, FeeReceivingContract>,

    // Account for TempTxStatusReportAccount PDA
    #[account(
        init,
        seeds = [b"temp_tx_status_report"],
        bump,
        payer = user,
        space = 10_240
    )]
    pub temp_report_account: Account<'info, TempTxStatusReportAccount>,

    // Account for ContributorDataAccount PDA
    #[account(
        init,
        seeds = [b"contributor_data"],
        bump,
        payer = user,
        space = 10_240
    )]
    pub contributor_data_account: Account<'info, ContributorDataAccount>,

    // Account for TxidSubmissionCountsAccount PDA
    #[account(
        init,
        seeds = [b"txid_submission_counts"],
        bump,
        payer = user,
        space = 10_240
    )]
    pub txid_submission_counts_account: Account<'info, TxidSubmissionCountsAccount>,

    // Account for AggregatedConsensusDataAccount PDA
    #[account(
        init,
        seeds = [b"aggregated_consensus_data"],
        bump,
        payer = user,
        space = 10_240
    )]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,

    // System program is needed for account creation
    pub system_program: Program<'info, System>,    
}

impl<'info> Initialize<'info> {
    pub fn initialize_oracle_state(&mut self, admin_pubkey: Pubkey) -> Result<()> {
        msg!("Setting up Oracle Contract State");

        let state = &mut self.oracle_contract_state;
        // Ensure the oracle_contract_state is not already initialized
        if state.is_initialized {
            return Err(OracleError::AccountAlreadyInitialized.into());
        }

        state.is_initialized = true;
        state.admin_pubkey = admin_pubkey;
        msg!("Admin Pubkey set to: {:?}", admin_pubkey);

        state.monitored_txids = Vec::new();
        msg!("Monitored Txids Vector initialized");

        state.bridge_contract_pubkey = Pubkey::default();
        msg!("Bridge Contract Pubkey set to default");

        msg!("Oracle Contract State Initialization Complete");
        Ok(())
    }
}


#[derive(Accounts)]
pub struct ReallocateOracleState<'info> {
    #[account(mut, has_one = admin_pubkey)]
    pub oracle_contract_state: Account<'info, OracleContractState>,
    pub admin_pubkey: Signer<'info>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub temp_report_account: Account<'info, TempTxStatusReportAccount>,
    #[account(mut)]
    pub contributor_data_account: Account<'info, ContributorDataAccount>,
    #[account(mut)]
    pub txid_submission_counts_account: Account<'info, TxidSubmissionCountsAccount>,
    #[account(mut)]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,
}

pub fn reallocate_temp_report_account(temp_report_account: &mut Account<'_, TempTxStatusReportAccount>) -> Result<()> {
    // Define the threshold at which to reallocate (e.g., 90% full)
    const REALLOCATION_THRESHOLD: f32 = 0.9;
    const ADDITIONAL_SPACE: usize = 10_240;
    const MAX_SIZE: usize = 100 * 1024;

    let current_size = temp_report_account.to_account_info().data_len();
    let current_usage = temp_report_account.reports.len() * std::mem::size_of::<TempTxStatusReport>();
    let usage_ratio = current_usage as f32 / current_size as f32;

    if usage_ratio > REALLOCATION_THRESHOLD {
        let new_size = std::cmp::min(current_size + ADDITIONAL_SPACE, MAX_SIZE);
        temp_report_account.to_account_info().realloc(new_size, false)?;
        msg!("TempTxStatusReportAccount reallocated to new size: {}", new_size);
    }
    
    Ok(())
}

pub fn reallocate_contributor_data_account(contributor_data_account: &mut Account<'_, ContributorDataAccount>) -> Result<()> {
    // Define the threshold at which to reallocate (e.g., 90% full)
    const REALLOCATION_THRESHOLD: f32 = 0.9;
    const ADDITIONAL_SPACE: usize = 10_240;
    const MAX_SIZE: usize = 100 * 1024;

    let current_size = contributor_data_account.to_account_info().data_len();
    let current_usage = contributor_data_account.contributors.len() * std::mem::size_of::<Contributor>();
    let usage_ratio = current_usage as f32 / current_size as f32;

    if usage_ratio > REALLOCATION_THRESHOLD {
        let new_size = std::cmp::min(current_size + ADDITIONAL_SPACE, MAX_SIZE);
        contributor_data_account.to_account_info().realloc(new_size, false)?;
        msg!("ContributorDataAccount reallocated to new size: {}", new_size);
    }
    
    Ok(())
}

pub fn reallocate_submission_counts_account(submission_counts_account: &mut Account<'_, TxidSubmissionCountsAccount>) -> Result<()> {
    // Define the threshold at which to reallocate (e.g., 90% full)
    const REALLOCATION_THRESHOLD: f32 = 0.9;
    const ADDITIONAL_SPACE: usize = 10_240;
    const MAX_SIZE: usize = 100 * 1024;

    let current_size = submission_counts_account.to_account_info().data_len();
    let current_usage = submission_counts_account.submission_counts.len() * std::mem::size_of::<TxidSubmissionCount>();
    let usage_ratio = current_usage as f32 / current_size as f32;

    if usage_ratio > REALLOCATION_THRESHOLD {
        let new_size = std::cmp::min(current_size + ADDITIONAL_SPACE, MAX_SIZE);
        submission_counts_account.to_account_info().realloc(new_size, false)?;
        msg!("TxidSubmissionCountsAccount reallocated to new size: {}", new_size);
    }
    
    Ok(())
}

pub fn reallocate_aggregated_consensus_data_account(aggregated_consensus_data_account: &mut Account<'_, AggregatedConsensusDataAccount>) -> Result<()> {
    // Define the threshold at which to reallocate (e.g., 90% full)
    const REALLOCATION_THRESHOLD: f32 = 0.9;
    const ADDITIONAL_SPACE: usize = 10_240;
    const MAX_SIZE: usize = 100 * 1024;

    let current_size = aggregated_consensus_data_account.to_account_info().data_len();
    let current_usage = aggregated_consensus_data_account.consensus_data.len() * std::mem::size_of::<AggregatedConsensusData>();
    let usage_ratio = current_usage as f32 / current_size as f32;

    if usage_ratio > REALLOCATION_THRESHOLD {
        let new_size = std::cmp::min(current_size + ADDITIONAL_SPACE, MAX_SIZE);
        aggregated_consensus_data_account.to_account_info().realloc(new_size, false)?;
        msg!("AggregatedConsensusDataAccount reallocated to new size: {}", new_size);
    }
    
    Ok(())
}

impl<'info> ReallocateOracleState<'info> {
    pub fn execute(ctx: Context<ReallocateOracleState>) -> Result<()> {
        let oracle_contract_state = &mut ctx.accounts.oracle_contract_state;

        // Calculate new size; add 10,240 bytes for each reallocation
        // Ensure not to exceed 100KB total size
        let current_size = oracle_contract_state.to_account_info().data_len();
        let additional_space = 10_240; // Increment size
        let max_size = 100 * 1024; // 100KB
        let new_size = std::cmp::min(current_size + additional_space, max_size);

        // Perform reallocation
        oracle_contract_state.to_account_info().realloc(new_size, false)?;

        msg!("OracleContractState reallocated to new size: {}", new_size);
        
        reallocate_temp_report_account(&mut ctx.accounts.temp_report_account)?;
        reallocate_contributor_data_account(&mut ctx.accounts.contributor_data_account)?;
        reallocate_submission_counts_account(&mut ctx.accounts.txid_submission_counts_account)?;
        reallocate_aggregated_consensus_data_account(&mut ctx.accounts.aggregated_consensus_data_account)?;
        Ok(())
    }
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct HashWeight {
    pub hash: String,
    pub weight: i32,
}

// Function to update hash weight
fn update_hash_weight(hash_weights: &mut Vec<HashWeight>, hash: &str, weight: i32) {
    let mut found = false;

    for hash_weight in hash_weights.iter_mut() {
        if hash_weight.hash.as_str() == hash {
            hash_weight.weight += weight;
            found = true;
            break;
        }
    }

    if !found {
        hash_weights.push(HashWeight {
            hash: hash.to_string(), // Clone only when necessary
            weight,
        });
    }
}

// Struct to hold aggregated data for consensus calculation
#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct AggregatedConsensusData {
    pub txid: String,
    pub status_weights: [i32; TXID_STATUS_VARIANT_COUNT],
    pub hash_weights: Vec<HashWeight>,
    pub first_6_characters_of_sha3_256_hash_of_corresponding_file: String,
    pub last_updated: u64, // Unix timestamp indicating the last update time
}


#[derive(Accounts)]
pub struct RequestReward<'info> {
    #[account(mut)]
    pub reward_pool_account: Account<'info, RewardPool>,
    #[account(mut)]
    pub oracle_contract_state: Account<'info, OracleContractState>,
    #[account(mut)]
    pub contributor_data_account: Account<'info, ContributorDataAccount>,
    pub system_program: Program<'info, System>,
}


pub fn request_reward_helper(ctx: Context<RequestReward>, contributor_address: Pubkey) -> Result<()> {

    // Temporarily store reward eligibility and amount
    let mut reward_amount = 0;
    let mut is_reward_valid = false;

    // Find the contributor in the PDA and check eligibility
    if let Some(contributor) = ctx.accounts.contributor_data_account.contributors.iter().find(|c| c.reward_address == contributor_address) {
        let current_unix_timestamp = Clock::get()?.unix_timestamp as u64;
        let is_eligible_for_rewards = contributor.is_eligible_for_rewards;
        let is_banned = contributor.calculate_is_banned(current_unix_timestamp);

        if is_eligible_for_rewards && !is_banned {
            reward_amount = BASE_REWARD_AMOUNT_IN_LAMPORTS; // Adjust based on your logic
            is_reward_valid = true;
        }
    } else {
        msg!("Contributor not found: {}", contributor_address);
        return Err(OracleError::UnregisteredOracle.into());
    }

    // Handle reward transfer after determining eligibility
    if is_reward_valid {
        // Transfer the reward from the reward pool to the contributor
        **ctx.accounts.reward_pool_account.to_account_info().lamports.borrow_mut() -= reward_amount;
        **ctx.accounts.oracle_contract_state.to_account_info().lamports.borrow_mut() += reward_amount;

        msg!("Paid out Valid Reward Request: Contributor: {}, Amount: {}", contributor_address, reward_amount);
    } else {

        msg!("Invalid Reward Request: Contributor: {}", contributor_address);
        return Err(OracleError::NotEligibleForReward.into());
    }

    Ok(())
}


#[derive(Accounts)]
pub struct RegisterNewDataContributor<'info> {

    /// CHECK: Manual checks are performed in the instruction to ensure the contributor_account is valid and safe to use.
    #[account(mut, signer)]
    pub contributor_account: AccountInfo<'info>,
    
    #[account(mut)]
    pub reward_pool_account: Account<'info, RewardPool>,

    #[account(mut)]
    pub fee_receiving_contract_account: Account<'info, FeeReceivingContract>,

    #[account(mut)]
    pub contributor_data_account: Account<'info, ContributorDataAccount>,

}


pub fn register_new_data_contributor_helper(ctx: Context<RegisterNewDataContributor>) -> Result<()> {
    let contributor_data_account = &mut ctx.accounts.contributor_data_account;
    msg!("Initiating new contributor registration: {}", ctx.accounts.contributor_account.key());

    // Check if the contributor is already registered
    if contributor_data_account.contributors.iter().any(|c| c.reward_address == *ctx.accounts.contributor_account.key) {
        msg!("Registration failed: Contributor already registered: {}", ctx.accounts.contributor_account.key);
        return Err(OracleError::ContributorAlreadyRegistered.into());
    }

    // Retrieve mutable references to the lamport balance
    let fee_receiving_account_info = ctx.accounts.fee_receiving_contract_account.to_account_info();
    let mut fee_receiving_account_lamports = fee_receiving_account_info.lamports.borrow_mut();

    let reward_pool_account_info = ctx.accounts.reward_pool_account.to_account_info();
    let mut reward_pool_account_lamports = reward_pool_account_info.lamports.borrow_mut();

    // Check if the fee_receiving_contract_account received the registration fee
    if **fee_receiving_account_lamports < REGISTRATION_ENTRANCE_FEE_IN_LAMPORTS as u64 {
        return Err(OracleError::RegistrationFeeNotPaid.into());
    }

    msg!("Registration fee verified. Attempting to register new contributor {}", ctx.accounts.contributor_account.key);

    // Deduct the registration fee from the fee_receiving_contract_account and add it to the reward pool account
    **fee_receiving_account_lamports -= REGISTRATION_ENTRANCE_FEE_IN_LAMPORTS as u64;
    **reward_pool_account_lamports += REGISTRATION_ENTRANCE_FEE_IN_LAMPORTS as u64;

    let last_active_timestamp = Clock::get()?.unix_timestamp as u64;
    
    // Create and add the new contributor
    let new_contributor = Contributor {
        reward_address: *ctx.accounts.contributor_account.key,
        registration_entrance_fee_transaction_signature: String::new(), // Replace with actual data if available
        compliance_score: 1.0, // Initial compliance score
        last_active_timestamp, // Set the last active timestamp to the current time
        total_reports_submitted: 0, // Initially, no reports have been submitted
        accurate_reports_count: 0, // Initially, no accurate reports
        current_streak: 0, // No streak at the beginning
        reliability_score: 1.0, // Initial reliability score
        consensus_failures: 0, // No consensus failures at the start
        ban_expiry: 0, // No ban initially set
        is_eligible_for_rewards: false, // Initially not eligible for rewards
        is_recently_active: false, // Initially not considered active
        is_reliable: false, // Initially not considered reliable
    };

    // Append the new contributor to the ContributorDataAccount
    contributor_data_account.contributors.push(new_contributor);

    // Logging for debug purposes
    msg!("New Contributor successfully Registered: Address: {}, Timestamp: {}", ctx.accounts.contributor_account.key, last_active_timestamp);
    Ok(())
}


#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct AddTxidForMonitoringData {
    pub txid: String,
}

#[derive(Accounts)]
pub struct AddTxidForMonitoring<'info> {
    #[account(mut)]
    pub oracle_contract_state: Account<'info, OracleContractState>,

    /// CHECK: The caller is manually verified in the instruction logic to ensure it's the correct and authorized account.
    #[account(signer)]
    pub caller: AccountInfo<'info>,

    // The `pending_payment_account` will be initialized in the function
    #[account(mut)]
    pub pending_payment_account: Account<'info, PendingPaymentAccount>,

    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}


pub fn add_txid_for_monitoring_helper(ctx: Context<AddTxidForMonitoring>, data: AddTxidForMonitoringData) -> Result<()> {
    let state = &mut ctx.accounts.oracle_contract_state;

    if ctx.accounts.caller.key != &state.bridge_contract_pubkey {
        return Err(OracleError::NotBridgeContractAddress.into());
    }

    // Explicitly cast txid to String and ensure it meets requirements
    let txid = data.txid.clone();
    if txid.len() > MAX_TXID_LENGTH {
        msg!("TXID exceeds maximum length.");
        return Err(OracleError::InvalidTxid.into());
    }

    // Add the TXID to the monitored list
    state.monitored_txids.push(txid.clone());

    // Initialize pending_payment_account here using the txid
    let pending_payment_account = &mut ctx.accounts.pending_payment_account;
    pending_payment_account.pending_payment = PendingPayment {
        txid: txid.clone(),
        expected_amount: COST_IN_LAMPORTS_OF_ADDING_PASTEL_TXID_FOR_MONITORING,
        payment_status: PaymentStatus::Pending, // Enum, no need for casting
    };

    msg!("Added Pastel TXID for Monitoring: {}", pending_payment_account.pending_payment.txid);
    Ok(())
}


#[derive(Accounts)]
pub struct ProcessPastelTxStatusReport<'info> {
    #[account(mut)]
    pub oracle_contract_state: Account<'info, OracleContractState>,

    /// CHECK: Manual checks are performed in the instruction to ensure the contributor is valid and authorized. This includes verifying signatures and other relevant validations.
    #[account(mut, signer)]
    pub contributor: AccountInfo<'info>,
    // You can add other accounts as needed
}

pub fn should_calculate_consensus(
    txid_submission_counts_account: &Account<TxidSubmissionCountsAccount>, 
    txid: &str
) -> Result<bool> {
    // Retrieve the count of submissions and last updated timestamp for the given txid
    let (submission_count, last_updated) = txid_submission_counts_account.submission_counts.iter()
        .find(|c| c.txid == txid)
        .map(|c| (c.count, c.last_updated))
        .unwrap_or((0, 0));

    // Check if the minimum threshold of reports is met
    let min_threshold_met = submission_count >= MIN_NUMBER_OF_ORACLES as u32;

    // Get the current unix timestamp from the Solana clock
    let current_unix_timestamp = Clock::get()?.unix_timestamp as u64;

    // Check if N minutes have elapsed since the last update
    let max_waiting_period_elapsed_for_txid = current_unix_timestamp - last_updated >= MAX_DURATION_IN_SECONDS_FROM_LAST_REPORT_SUBMISSION_BEFORE_COMPUTING_CONSENSUS;

    // Calculate consensus if minimum threshold is met or if N minutes have passed with at least MIN_NUMBER_OF_ORACLES reports
    Ok(min_threshold_met || (max_waiting_period_elapsed_for_txid && submission_count >= MIN_NUMBER_OF_ORACLES as u32))
}

pub fn cleanup_old_submission_counts(state: &mut OracleContractState) -> Result<()> {
    let current_time = Clock::get()?.unix_timestamp as u64;
    state.txid_submission_counts.retain(|count| {
        current_time - count.last_updated < SUBMISSION_COUNT_RETENTION_PERIOD
    });
    Ok(())
}

pub fn usize_to_txid_status(index: usize) -> Option<TxidStatus> {
    match index {
        0 => Some(TxidStatus::Invalid),
        1 => Some(TxidStatus::PendingMining),
        2 => Some(TxidStatus::MinedPendingActivation),
        3 => Some(TxidStatus::MinedActivated),
        _ => None,
    }
}


// Function to handle the submission of Pastel transaction status reports
pub fn validate_data_contributor_report(report: &PastelTxStatusReport) -> Result<()> {
    // Direct return in case of invalid data, reducing nested if conditions
    if report.txid.trim().is_empty() {
        msg!("Error: InvalidTxid (TXID is empty)");
        return Err(OracleError::InvalidTxid.into());
    } 
    // Simplified TXID status validation
    if !matches!(report.txid_status, TxidStatus::MinedActivated | TxidStatus::MinedPendingActivation | TxidStatus::PendingMining | TxidStatus::Invalid) {
        return Err(OracleError::InvalidTxidStatus.into());
    }
    // Direct return in case of missing data, reducing nested if conditions
    if report.pastel_ticket_type.is_none() {
        msg!("Error: Missing Pastel Ticket Type");
        return Err(OracleError::MissingPastelTicketType.into());
    }
    // Direct return in case of invalid hash, reducing nested if conditions
    if let Some(hash) = &report.first_6_characters_of_sha3_256_hash_of_corresponding_file {
        if hash.len() != 6 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            msg!("Error: Invalid File Hash Length or Non-hex characters");
            return Err(OracleError::InvalidFileHashLength.into());
        }
    } else {
        return Err(OracleError::MissingFileHash.into());
    }
    Ok(())
}


impl Contributor {

    // Check if the contributor is currently banned
    pub fn calculate_is_banned(&self, current_time: u64) -> bool {
        current_time < self.ban_expiry
    }

    // Method to determine if the contributor is eligible for rewards
    pub fn calculate_is_eligible_for_rewards(&self) -> bool {
        self.total_reports_submitted >= MIN_REPORTS_FOR_REWARD 
            && self.reliability_score >= MIN_RELIABILITY_SCORE_FOR_REWARD 
            && self.compliance_score >= MIN_COMPLIANCE_SCORE_FOR_REWARD
    }

}


#[derive(Accounts)]
pub struct SetBridgeContract<'info> {
    #[account(mut, has_one = admin_pubkey)]
    pub oracle_contract_state: Account<'info, OracleContractState>,
    pub admin_pubkey: Signer<'info>,
}

impl<'info> SetBridgeContract<'info> {
    pub fn set_bridge_contract(ctx: Context<SetBridgeContract>, bridge_contract_pubkey: Pubkey) -> Result<()> {
        let state = &mut ctx.accounts.oracle_contract_state;
        state.bridge_contract_pubkey = bridge_contract_pubkey;
        msg!("Bridge contract pubkey updated: {:?}", bridge_contract_pubkey);
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(txid: String)] // Include txid as part of the instruction
pub struct ProcessPayment<'info> {
    /// CHECK: This is checked in the handler function to verify it's the bridge contract.
    #[account(signer)]
    pub source_account: AccountInfo<'info>,

    #[account(mut)]
    pub oracle_contract_state: Account<'info, OracleContractState>,

    #[account(
        mut,
        seeds = [create_seed("pending_payment", &txid, &source_account.key()).as_ref()],
        bump // You won't explicitly include the bump here; it's handled by Anchor
    )]
    pub pending_payment_account: Account<'info, PendingPaymentAccount>,

    pub system_program: Program<'info, System>,
}


pub fn process_payment_helper(
    ctx: Context<ProcessPayment>, 
    txid: String, 
    amount: u64
) -> Result<()> {
    // Access the pending payment account using the txid as a seed
    let pending_payment_account = &mut ctx.accounts.pending_payment_account;

    // Ensure the payment corresponds to the provided txid
    if pending_payment_account.pending_payment.txid != txid {
        return Err(OracleError::PaymentNotFound.into());
    }

    // Verify the payment amount matches the expected amount
    if pending_payment_account.pending_payment.expected_amount != amount {
        return Err(OracleError::InvalidPaymentAmount.into());
    }

    // Mark the payment as received
    pending_payment_account.pending_payment.payment_status = PaymentStatus::Received;

    Ok(())
}


#[derive(Accounts)]
pub struct WithdrawFunds<'info> {
    #[account(
        mut,
        constraint = oracle_contract_state.admin_pubkey == *admin_account.key @ OracleError::UnauthorizedWithdrawalAccount,
    )]
    pub oracle_contract_state: Account<'info, OracleContractState>,

    /// CHECK: The admin_account is manually verified in the instruction to ensure it's the correct and authorized account for withdrawal operations. This includes checking if the account matches the admin_pubkey stored in oracle_contract_state.
    pub admin_account: AccountInfo<'info>,

    #[account(mut)]
    pub reward_pool_account: Account<'info, RewardPool>,
    #[account(mut)]
    pub fee_receiving_contract_account: Account<'info, FeeReceivingContract>,
    pub system_program: Program<'info, System>,
}

impl<'info> WithdrawFunds<'info> {
    pub fn execute(ctx: Context<WithdrawFunds>, reward_pool_amount: u64, fee_receiving_amount: u64) -> Result<()> {
        if !ctx.accounts.admin_account.is_signer {
            return Err(OracleError::UnauthorizedWithdrawalAccount.into()); // Check if the admin_account is a signer
        } 
        let admin_account = &mut ctx.accounts.admin_account;
        let reward_pool_account = &mut ctx.accounts.reward_pool_account;
        let fee_receiving_contract_account = &mut ctx.accounts.fee_receiving_contract_account;

        // Transfer SOL from the reward pool account to the admin account
        if **reward_pool_account.to_account_info().lamports.borrow() < reward_pool_amount {
            return Err(OracleError::InsufficientFunds.into());
        }
        **reward_pool_account.to_account_info().lamports.borrow_mut() -= reward_pool_amount;
        **admin_account.lamports.borrow_mut() += reward_pool_amount;

        // Transfer SOL from the fee receiving contract account to the admin account
        if **fee_receiving_contract_account.to_account_info().lamports.borrow() < fee_receiving_amount {
            return Err(OracleError::InsufficientFunds.into());
        }
        **fee_receiving_contract_account.to_account_info().lamports.borrow_mut() -= fee_receiving_amount;
        **admin_account.lamports.borrow_mut() += fee_receiving_amount;

        msg!("Withdrawal successful: {} lamports transferred from reward pool and {} lamports from fee receiving contract to admin account", reward_pool_amount, fee_receiving_amount);
        Ok(())
    }
}


declare_id!("AfP1c4sFcY1FeiGjQEtyxCim8BRnw22okNbKAsH2sBsB");

pub mod solana_pastel_oracle_program {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, admin_pubkey: Pubkey) -> Result<()> {
        msg!("Initializing Oracle Contract State");
        ctx.accounts.initialize_oracle_state(admin_pubkey)?;
        msg!("Oracle Contract State Initialized with Admin Pubkey: {:?}", admin_pubkey);
    
        // Logging for Reward Pool and Fee Receiving Contract Accounts
        msg!("Reward Pool Account: {:?}", ctx.accounts.reward_pool_account.key());
        msg!("Fee Receiving Contract Account: {:?}", ctx.accounts.fee_receiving_contract_account.key());
        msg!("Temp Report Account: {:?}", ctx.accounts.temp_report_account.key());
        msg!("Contributor Data Account: {:?}", ctx.accounts.contributor_data_account.key());
        msg!("Txid Submission Counts Account: {:?}", ctx.accounts.txid_submission_counts_account.key());
        msg!("Aggregated Consensus Data Account: {:?}", ctx.accounts.aggregated_consensus_data_account.key());
    
        Ok(())
    }
    
    pub fn reallocate_oracle_state(ctx: Context<ReallocateOracleState>) -> Result<()> {
        ReallocateOracleState::execute(ctx)
    }

    pub fn register_new_data_contributor(ctx: Context<RegisterNewDataContributor>) -> Result<()> {
        register_new_data_contributor_helper(ctx)
    }

    pub fn add_txid_for_monitoring(ctx: Context<AddTxidForMonitoring>, data: AddTxidForMonitoringData) -> Result<()> {
        add_txid_for_monitoring_helper(ctx, data)
    }

    pub fn add_pending_payment(ctx: Context<HandlePendingPayment>, txid: String, expected_amount_str: String, payment_status_str: String) -> Result<()> {
        let expected_amount = expected_amount_str.parse::<u64>()
            .map_err(|_| OracleError::PendingPaymentInvalidAmount)?;
    
        // Convert the payment status from string to enum
        let payment_status = match payment_status_str.as_str() {
            "Pending" => PaymentStatus::Pending,
            "Received" => PaymentStatus::Received,
            _ => return Err(OracleError::InvalidPaymentStatus.into()),
        };
    
        let pending_payment = PendingPayment {
            txid: txid.clone(),
            expected_amount,
            payment_status,
        };
    
        add_pending_payment_helper(ctx, txid, pending_payment)
            .map_err(|e| e.into())
    }
    
    
    pub fn process_payment(ctx: Context<ProcessPayment>, txid: String, amount: u64) -> Result<()> {
        process_payment_helper(ctx, txid, amount)
    }

    pub fn submit_data_report(
        ctx: Context<SubmitDataReport>, 
        txid: String, 
        txid_status_str: String, 
        pastel_ticket_type_str: String, 
        first_6_characters_hash: String, 
        contributor_reward_address: Pubkey
    ) -> ProgramResult {
        msg!("In `submit_data_report` function -- Params: txid={}, txid_status_str={}, pastel_ticket_type_str={}, first_6_chars_hash={}, contributor_addr={}",
            txid, txid_status_str, pastel_ticket_type_str, first_6_characters_hash, contributor_reward_address);
    
        // Conversion logic remains the same
        let txid_status = match txid_status_str.as_str() {
            "Invalid" => TxidStatus::Invalid,
            "PendingMining" => TxidStatus::PendingMining,
            "MinedPendingActivation" => TxidStatus::MinedPendingActivation,
            "MinedActivated" => TxidStatus::MinedActivated,
            _ => return Err(ProgramError::from(OracleError::InvalidTxidStatus))
        };
    
        let pastel_ticket_type = match pastel_ticket_type_str.as_str() {
            "Sense" => PastelTicketType::Sense,
            "Cascade" => PastelTicketType::Cascade,
            "Nft" => PastelTicketType::Nft,
            "InferenceApi" => PastelTicketType::InferenceApi,
            _ => return Err(ProgramError::from(OracleError::InvalidPastelTicketType))
        };
    
        let timestamp = Clock::get()?.unix_timestamp as u64;
    
        let report = PastelTxStatusReport {
            txid: txid.clone(),
            txid_status,
            pastel_ticket_type: Some(pastel_ticket_type),
            first_6_characters_of_sha3_256_hash_of_corresponding_file: Some(first_6_characters_hash),
            timestamp,
            contributor_reward_address,
        };
    
        submit_data_report_helper(ctx, txid, report, contributor_reward_address)
    }
    
    pub fn request_reward(ctx: Context<RequestReward>, contributor_address: Pubkey) -> Result<()> {
        request_reward_helper(ctx, contributor_address)
    }

    pub fn set_bridge_contract(ctx: Context<SetBridgeContract>, bridge_contract_pubkey: Pubkey) -> Result<()> {
        SetBridgeContract::set_bridge_contract(ctx, bridge_contract_pubkey)
    }

    pub fn withdraw_funds(ctx: Context<WithdrawFunds>, reward_pool_amount: u64, fee_receiving_amount: u64) -> Result<()> {
        WithdrawFunds::execute(ctx, reward_pool_amount, fee_receiving_amount)
    }

}