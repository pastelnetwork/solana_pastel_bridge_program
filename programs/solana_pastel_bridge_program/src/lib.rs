use anchor_lang::prelude::*;
use anchor_lang::solana_program::entrypoint::ProgramResult;
use anchor_lang::solana_program::account_info::AccountInfo;
use anchor_lang::solana_program::sysvar::clock::Clock;
use anchor_lang::solana_program::hash::hash;
use anchor_lang::solana_program::program::invoke;
use anchor_lang::solana_program::system_instruction;

const MAX_QUOTE_RESPONSE_TIME: u64 = 600; // Max time for bridge nodes to respond with a quote in seconds (10 minutes)
const QUOTE_VALIDITY_DURATION: u64 = 21_600; // The time for which a submitted price quote remains valid. e.g., 6 hours in seconds
const ESCROW_DURATION: u64 = 7_200; // Duration to hold SOL in escrow in seconds (2 hours); if the service request is not fulfilled within this time, the SOL is refunded to the user and the bridge node won't receive any payment even if they fulfill the request later.
const DURATION_IN_SECONDS_TO_WAIT_AFTER_ANNOUNCING_NEW_PENDING_SERVICE_REQUEST_BEFORE_SELECTING_BEST_QUOTE: u64 = 300; // amount of time to wait after advertising pending service requests to select the best quote
const TRANSACTION_FEE_PERCENTAGE: u8 = 2; // Percentage of the selected (best) quoted price in SOL to be retained as a fee by the bridge contract for each successfully completed service request; the rest should be paid to the bridge node. If the bridge node fails to fulfill the service request, the full escrow  is refunded to the user without any deduction for the service fee. 
const BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS: u64 = 1_000_000; // Registration fee for bridge nodes in lamports
const SERVICE_REQUEST_VALIDITY: u64 = 86_400; // Time until a service request expires if not responded to. e.g., 24 hours in seconds
const BRIDGE_NODE_INACTIVITY_THRESHOLD: u64 = 86_400; // e.g., 24 hours in seconds
const MIN_COMPLIANCE_SCORE_FOR_REWARD: f32 = 65.0; // Bridge Node must have a compliance score of at least N to be eligible for rewards
const MIN_RELIABILITY_SCORE_FOR_REWARD: f32 = 80.0; // Minimum reliability score to be eligible for rewards
const SERVICE_REQUESTS_FOR_PERMANENT_BAN: u32 = 250; // 
const SERVICE_REQUESTS_FOR_TEMPORARY_BAN: u32 = 50; // Considered for temporary ban after 50 service requests
const TEMPORARY_BAN_SERVICE_FAILURES_THRESHOLD: u32 = 5; // Number of non-consensus report submissions for temporary ban
const TEMPORARY_BAN_DURATION: u64 =  24 * 60 * 60; // Duration of temporary ban in seconds (e.g., 1 day)
const MAX_DURATION_IN_SECONDS_FROM_LAST_REPORT_SUBMISSION_BEFORE_SELECTING_WINNING_QUOTE: u64 = 2 * 60; // Maximum duration in seconds from service quote request before selecting the best quote (e.g., 2 minutes)
const DATA_RETENTION_PERIOD: u64 = 24 * 60 * 60; // How long to keep data in the contract state (1 day)
const TXID_STATUS_VARIANT_COUNT: usize = 4; // Manually define the number of variants in TxidStatus
const MAX_TXID_LENGTH: usize = 64; // Maximum length of a TXID
const MAX_ALLOWED_TIMESTAMP_DIFFERENCE: u64 = 600; // 10 minutes in seconds; maximum allowed time difference between the last update of the oracle's consensus data and the bridge contract's access to this data to ensure the bridge contract acts on timely and accurate data.
const LAMPORTS_PER_SOL: u64 = 1_000_000_000; // Number of lamports in one SOL


#[error_code]
pub enum BridgeError {
    #[msg("Bridge Contract state is already initialized")]
    ContractStateAlreadyInitialized,

    #[msg("Bridge node is already registered")]
    BridgeNodeAlreadyRegistered,

    #[msg("Action attempted by an unregistered bridge node")]
    UnregisteredBridgeNode,

    #[msg("Service request is invalid or malformed")]
    InvalidServiceRequest,

    #[msg("Escrow account for service request is not adequately funded")]
    EscrowNotFunded,

    #[msg("File size exceeds the maximum allowed limit")]
    InvalidFileSize,

    #[msg("Invalid or unsupported service type in service request")]
    InvalidServiceType,

    #[msg("Submitted price quote has expired and is no longer valid")]
    QuoteExpired,

    #[msg("Bridge node is inactive based on defined inactivity threshold")]
    BridgeNodeInactive,

    #[msg("Insufficient funds in escrow to cover the transaction")]
    InsufficientEscrowFunds,

    #[msg("Duplicate service request ID")]
    DuplicateServiceRequestId,

    #[msg("Contract is paused and no operations are allowed")]
    ContractPaused,

    #[msg("Invalid or missing first 6 characters of SHA3-256 hash")]
    InvalidFileHash,

    #[msg("IPFS CID is empty")]
    InvalidIpfsCid,

    #[msg("User Solana address is missing or invalid")]
    InvalidUserSolAddress,

    #[msg("Initial service request status must be Pending")]
    InvalidRequestStatus,

    #[msg("Payment in escrow should initially be false")]
    InvalidPaymentInEscrow,

    #[msg("Initial values for certain fields must be None")]
    InvalidInitialFieldValues,

    #[msg("Initial escrow amount and fees must be None or zero")]
    InvalidEscrowOrFeeAmounts,

    #[msg("Invalid service request ID.")]
    InvalidServiceRequestId,

    #[msg("Invalid Pastel transaction ID.")]
    InvalidPastelTxid,

    #[msg("Bridge node has not been selected for the service request.")]
    BridgeNodeNotSelected,

    #[msg("Unauthorized bridge node.")]
    UnauthorizedBridgeNode,

    #[msg("Bridge contract is not initialized.")]
    ContractNotInitialized,    

    #[msg("Service request to Pastel transaction ID mapping not found")]
    MappingNotFound,

    #[msg("Pastel transaction ID does not match with any service request")]
    TxidMismatch,

    #[msg("Consensus data from the oracle is outdated")]
    OutdatedConsensusData,    

    #[msg("Service request not found")]
    ServiceRequestNotFound,

    #[msg("Service request is not in a pending state")]
    ServiceRequestNotPending,

    #[msg("Price quote submitted too late")]
    QuoteResponseTimeExceeded,

    #[msg("Bridge node is currently banned")]
    BridgeNodeBanned,

    #[msg("Quoted price cannot be zero")]
    InvalidQuotedPrice,

    #[msg("Price quote status is not set to 'Submitted'")]
    InvalidQuoteStatus,

    #[msg("Bridge node does not meet the minimum score requirements for rewards")]
    BridgeNodeScoreTooLow,

    #[msg("Pastel TXID not found in oracle data")]
    TxidNotFound,

    #[msg("Txid to Service Request ID Mapping account not found")]
    TxidMappingNotFound,

    #[msg("Insufficient registration fee paid by bridge node")]
    InsufficientRegistrationFee,

    #[msg("Withdrawal request from unauthorized account")]
    UnauthorizedWithdrawalAccount,

    #[msg("Insufficient funds for withdrawal request")]
    InsufficientFunds,
    
    #[msg("Timestamp conversion error")]
    TimestampConversionError,

    #[msg("Registration fee not paid")]
    RegistrationFeeNotPaid,    
}

impl From<BridgeError> for ProgramError {
    fn from(e: BridgeError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

// Enums:

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, AnchorSerialize, AnchorDeserialize)]
pub enum TxidStatus {
    Invalid,
    PendingMining,
    MinedPendingActivation,
    MinedActivated,
}

//These are the different kinds of service requests that can be submitted to the bridge contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, AnchorSerialize, AnchorDeserialize)]
pub enum PastelTicketType {
    Sense,
    Cascade,
    Nft,
    InferenceApi,
}

// This tracks the status of an individual price quote submitted by a bridge node a for service request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, AnchorSerialize, AnchorDeserialize)]
pub enum ServicePriceQuoteStatus {
    Submitted,
    RejectedAsInvalid,
    RejectedAsTooHigh,
    Accepted
}

// This tracks the status of the selection of the best price quote for a particular service request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, AnchorSerialize, AnchorDeserialize)]
pub enum BestQuoteSelectionStatus {
    NoQuotesReceivedYet,
    NoValidQuotesReceivedYet,
    WaitingToSelectBestQuote,
    BestQuoteSelected,
}

// This tracks the status of Solana payments for service requests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, AnchorSerialize, AnchorDeserialize)]
pub enum PaymentStatus {
    Pending,
    Received
}

// This controls emergency actions that can be taken by the admin to pause operations, modify parameters, etc. in the bridge contract in the event of an emergency.
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub enum EmergencyAction {
    PauseOperations, // Pause all contract operations.
    ResumeOperations, // Resume all contract operations.
    ModifyParameters { key: String, value: String }, // Modify certain operational parameters.
    // Additional emergency actions as needed...
}

// These are the various states that a service request can assume during its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, AnchorSerialize, AnchorDeserialize)]
pub enum RequestStatus {
    Pending, // The request has been created and is awaiting further action.
    AwaitingPayment, // The request is waiting for SOL payment from the user.
    PaymentReceived, // Payment for the request has been received and is held in escrow.
    BridgeNodeSelected, // A bridge node has been selected to fulfill the service request.
    InProgress, // The service is currently being rendered by the selected bridge node.
    AwaitingCompletionConfirmation, // The service has been completed and is awaiting confirmation from the oracle.
    Completed, // The service has been successfully completed and confirmed.
    Failed, // The service request has failed or encountered an error.
    Expired, // The request has expired due to inactivity or non-fulfillment.
    Refunded, // Indicates that the request has been refunded to the user.
}

// The nodes that perform the service requests on behalf of the end users are called bridge nodes.
#[derive(Debug, Clone, PartialEq, AnchorSerialize, AnchorDeserialize)]
pub struct BridgeNode {
    pub pastel_id: String, // The unique identifier of the bridge node in the Pastel network, used as the primary key throughout the bridge contract to identify the bridge node.
    pub reward_address: Pubkey, // The Solana address of the bridge node, used to send rewards to the bridge node.
    pub bridge_node_psl_address: String, // The Pastel address of the bridge node, used to pay for service requests made by the bridge node on behalf of end users.
    pub registration_entrance_fee_transaction_signature: String, // The signature of the transaction that paid the registration fee in SOL for the bridge node to register with the bridge contract.
    pub compliance_score: f32, // The compliance score of the bridge node, which is a combined measure of the bridge node's overall track record of performing services quickly, accurately, and reliably.
    pub reliability_score: f32, // The reliability score of the bridge node, which is the percentage of all attempted service requests that were completed successfully by the bridge node.
    pub last_active_timestamp: u64, // The timestamp of the last time the bridge node performed a service request.
    pub total_price_quotes_submitted: u32, // The total number of price quotes submitted by the bridge node since registration.
    pub total_service_requests_attempted: u32, // The total number of service requests attempted by the bridge node since registration.
    pub successful_service_requests_count: u32, // The total number of service requests successfully completed by the bridge node since registration.
    pub current_streak: u32, // The current number of consecutive service requests successfully completed by the bridge node.
    pub failed_service_requests_count: u32, // The total number of service requests attempted by the bridge node that failed or were not completed successfully for any reason (even if not the bridge node's fault).
    pub ban_expiry: u64, // The timestamp when the bridge node's ban expires, if applicable.
    pub is_eligible_for_rewards: bool, // Indicates if the bridge node is eligible for rewards.
    pub is_recently_active: bool, // Indicates if the bridge node has been active recently.
    pub is_reliable: bool, // Indicates if the bridge node is considered reliable based on its track record.   
    pub is_banned: bool, // Indicates if the bridge node is currently banned.
}

// These are the requests for services on Pastel Network submitted by end users; they are stored in the active service requests account.
// These requests initially come in with the type of service requested, the file hash, the IPFS CID, the file size, and the file MIME type;
// Then the bridge nodes submit price quotes for the service request, and the contract selects the best quote and selects a bridge node to fulfill the request.
// The end user then pays the quoted amount in SOL to the Bridge contract, which holds this amount in escrow until the service is completed successfully.
// The selected bridge node then performs the service and submits the Pastel transaction ID to the bridge contract when it's available; the bridge node ALSO submits the same
// Pastel transaction ID (txid) to the oracle contract for monitoring. When the oracle contract confirms the status of the transaction as being mined and activated,
// the bridge contract confirms that the ticket has been activated and that the file referenced in the ticket matches the file hash submitted in the
// service request. If the file hash matches, the escrowed SOL is released to the bridge node (minus the service fee paid to the Bridge contract) and the service request is marked as completed.
// If the file hash does not match, or if the oracle contract does not confirm the transaction as being mined and activated within the specified time limit, the escrowed SOL is refunded to the end user.
// If the bridge node fails to submit the Pastel transaction ID within the specified time limit, the service request is marked as failed and the escrowed SOL is refunded to the end user.
// The retained service fee (assuming a successful service request) is distributed to the the bridge contract's reward pool and the selected bridge node's scores are then
// updated based on the outcome of the service request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub struct ServiceRequest {
    pub service_request_id: String, // Unique identifier for the service request; SHA256 hash of (service_type_as_str + first_6_characters_of_sha3_256_hash_of_corresponding_file + user_sol_address) expressed as a 32-byte array.
    pub service_type: PastelTicketType, // Type of service requested (e.g., Sense, Cascade, Nft).
    pub first_6_characters_of_sha3_256_hash_of_corresponding_file: String, // First 6 characters of the SHA3-256 hash of the file involved in the service request.
    pub ipfs_cid: String, // IPFS Content Identifier for the file.
    pub file_size_bytes: u64, // Size of the file in bytes.
    pub user_sol_address: Pubkey, // Solana address of the end user who initiated the service request.
    pub status: RequestStatus, // Current status of the service request.
    pub payment_in_escrow: bool, // Indicates if the payment for the service is currently in escrow.
    pub request_expiry: u64, // Timestamp when the service request expires.
    pub sol_received_from_user_timestamp: Option<u64>, // Timestamp when SOL payment is received from the end user for the service request to be held in escrow.
    pub selected_bridge_node_pastelid: Option<String>, // The Pastel ID of the bridge node selected to fulfill the service request.
    pub best_quoted_price_in_lamports: Option<u64>, // Price quoted for the service in lamports.
    pub service_request_creation_timestamp: u64, // Timestamp when the service request was created.
    pub bridge_node_selection_timestamp: Option<u64>, // Timestamp when a bridge node was selected for the service.
    pub bridge_node_submission_of_txid_timestamp: Option<u64>, // Timestamp when the bridge node submitted the Pastel transaction ID for the service request.
    pub submission_of_txid_to_oracle_timestamp: Option<u64>, // Timestamp when the Pastel transaction ID was submitted by the bridge contract to the oracle contract for monitoring.
    pub service_request_completion_timestamp: Option<u64>, // Timestamp when the service was completed.
    pub payment_received_timestamp: Option<u64>, // Timestamp when the payment was received into escrow.
    pub payment_release_timestamp: Option<u64>, // Timestamp when the payment was released from escrow, if applicable.    
    pub escrow_amount_lamports: Option<u64>, // Amount of SOL held in escrow for this service request.
    pub service_fee_retained_by_bridge_contract_lamports: Option<u64>, // Amount of SOL retained by the bridge contract as a service fee, taken from the escrowed amount when the service request is completed.
    pub pastel_txid: Option<String>, // The Pastel transaction ID for the service request once it is created.
}

// This holds the information for an individual price quote from a given bridge node for a particular service request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub struct ServicePriceQuote {
    pub service_request_id: String,
    pub bridge_node_pastel_id: String,
    pub quoted_price_lamports: u64,
    pub quote_timestamp: u64,
    pub price_quote_status: ServicePriceQuoteStatus,
}

// Struct to hold final consensus of the txid's status from the oracle contract
#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct AggregatedConsensusData {
    pub txid: String,
    pub status_weights: [i32; TXID_STATUS_VARIANT_COUNT],
    pub hash_weights: Vec<HashWeight>,
    pub first_6_characters_of_sha3_256_hash_of_corresponding_file: String,
    pub last_updated: u64, // Unix timestamp indicating the last update time
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct ServiceRequestTxidMapping {
    pub service_request_id: String,
    pub pastel_txid: String,
}

// Account to receive registration fees from bridge nodes before it is transferred to the bridge contract reward pool account 
#[account]
pub struct RegFeeReceivingAccount {}

#[account]
pub struct BridgeContractState {
    pub is_initialized: bool,
    pub is_paused: bool,
    pub admin_pubkey: Pubkey,
    pub oracle_contract_pubkey: Pubkey,    
    pub bridge_reward_pool_account_pubkey: Pubkey,
    pub bridge_escrow_account_pubkey: Pubkey,
    pub bridge_nodes_data_account_pubkey: Pubkey,
    pub temp_service_requests_data_account_pubkey: Pubkey,
    pub aggregated_consensus_data_account_pubkey: Pubkey,
    pub service_request_txid_mapping_account_pubkey: Pubkey,
    pub reg_fee_receiving_account_pubkey: Pubkey,     
}

// PDA that distributes rewards to bridge nodes for fulfilling service requests successfully
#[account]
pub struct BridgeRewardPoolAccount {
    // Since this account is only used for holding and transferring SOL, no fields are necessary.
}

// PDA that acts as a temporary escrow account for holding SOL while service requests are being processed
#[account]
pub struct BridgeEscrowAccount {
    // Since this account is only used for holding and transferring SOL, no fields are necessary.
}

// PDA to hold the list of bridge nodes and their details
#[account]
pub struct BridgeNodesDataAccount {
    pub bridge_nodes: Vec<BridgeNode>,
}

// PDA to hold the corresponding txid for each service request id
#[account]
pub struct ServiceRequestTxidMappingDataAccount {
    pub mappings: Vec<ServiceRequestTxidMapping>,
}

// Temporary account to hold service requests while they are being processed; these are cleared out after completion or expiration.
#[account]
pub struct TempServiceRequestsDataAccount {
    pub service_requests: Vec<ServiceRequest>,
}

// Account to hold the aggregated consensus data about the status of service request TXIDs from the oracle contract; this is cleared out periodically.
#[account]
pub struct AggregatedConsensusDataAccount {
    pub consensus_data: Vec<AggregatedConsensusData>,
}

// Main bridge contract state account that holds all the data for the bridge contract
#[derive(Accounts)]
#[instruction(admin_pubkey: Pubkey)]
pub struct Initialize<'info> {
    #[account(init, payer = user, space = 10_240)] // Adjusted space
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(init, seeds = [b"bridge_reward_pool_account"], bump, payer = user, space = 1024)]
    pub bridge_reward_pool_account: Account<'info, BridgeRewardPoolAccount>,

    #[account(init, seeds = [b"reg_fee_receiving_account"], bump, payer = user, space = 1024)]
    pub reg_fee_receiving_account: Account<'info, RegFeeReceivingAccount>,

    #[account(init, seeds = [b"bridge_escrow_account"], bump, payer = user, space = 1024)]
    pub bridge_escrow_account: Account<'info, BridgeEscrowAccount>,

    #[account(init, seeds = [b"bridge_nodes_data"], bump, payer = user, space = 10_240)]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    #[account(init, seeds = [b"temp_service_requests_data"], bump, payer = user, space = 10_240)]
    pub temp_service_requests_data_account: Account<'info, TempServiceRequestsDataAccount>,

    #[account(init, seeds = [b"aggregated_consensus_data"], bump, payer = user, space = 10_240)]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,

    #[account(init, seeds = [b"service_request_txid_map"], bump, payer = user, space = 10_240)]
    pub service_request_txid_mapping_data_account: Account<'info, ServiceRequestTxidMappingDataAccount>,

    // System program is needed for account creation
    pub system_program: Program<'info, System>,    
}

impl<'info> Initialize<'info> {
    pub fn initialize_bridge_state(&mut self, admin_pubkey: Pubkey) -> Result<()> {
        msg!("Setting up Bridge Contract State");

        let state = &mut self.bridge_contract_state;
        // Ensure the bridge_contract_state is not already initialized
        if state.is_initialized {
            return Err(BridgeError::ContractStateAlreadyInitialized.into());
        }
        state.is_initialized = true;
        state.is_paused = false;
        state.admin_pubkey = admin_pubkey;
        msg!("Admin Pubkey set to: {:?}", admin_pubkey);

        // Link to necessary PDAs
        state.bridge_reward_pool_account_pubkey = self.bridge_reward_pool_account.key();
        state.bridge_escrow_account_pubkey = self.bridge_escrow_account.key();
        state.bridge_nodes_data_account_pubkey = self.bridge_nodes_data_account.key();
        state.temp_service_requests_data_account_pubkey = self.temp_service_requests_data_account.key();
        state.aggregated_consensus_data_account_pubkey = self.aggregated_consensus_data_account.key();
        state.reg_fee_receiving_account_pubkey = self.reg_fee_receiving_account.key();

        state.oracle_contract_pubkey = Pubkey::default();
        msg!("Oracle Contract Pubkey set to default");
        // Initialize bridge nodes data PDA
        let bridge_nodes_data_account = &mut self.bridge_nodes_data_account;
        bridge_nodes_data_account.bridge_nodes = Vec::new();

        // Initialize temporary service requests data PDA
        let temp_service_requests_data_account = &mut self.temp_service_requests_data_account;
        temp_service_requests_data_account.service_requests = Vec::new();

        // Initialize aggregated consensus data PDA
        let aggregated_consensus_data_account = &mut self.aggregated_consensus_data_account;
        aggregated_consensus_data_account.consensus_data = Vec::new();

        // Initialize service request TXID mapping data PDA
        let service_request_txid_mapping_data_account = &mut self.service_request_txid_mapping_data_account;
        service_request_txid_mapping_data_account.mappings = Vec::new();
    
        msg!("Bridge Contract State Initialization Complete");
        Ok(())
    }
}


#[derive(Accounts)]
pub struct ReallocateBridgeState<'info> {
    #[account(mut, has_one = admin_pubkey)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,
    pub admin_pubkey: Signer<'info>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub temp_service_requests_data_account: Account<'info, TempServiceRequestsDataAccount>,
    #[account(mut)]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,
    #[account(mut)]
    pub service_request_txid_mapping_data_account: Account<'info, ServiceRequestTxidMappingDataAccount>,
    #[account(mut)]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,
}

// Define the reallocation thresholds and limits
const REALLOCATION_THRESHOLD: f32 = 0.9;
const ADDITIONAL_SPACE: usize = 10_240;
const MAX_SIZE: usize = 100 * 1024; // 100KB

// Function to reallocate account data
fn reallocate_account_data(account: &mut AccountInfo, current_usage: usize) -> Result<()> {
    let current_size = account.data_len();
    let usage_ratio = current_usage as f32 / current_size as f32;

    if usage_ratio > REALLOCATION_THRESHOLD {
        let new_size = std::cmp::min(current_size + ADDITIONAL_SPACE, MAX_SIZE);
        account.realloc(new_size, false)?;
        msg!("Account reallocated to new size: {}", new_size);
    }

    Ok(())
}


impl<'info> ReallocateBridgeState<'info> {
    pub fn execute(ctx: Context<ReallocateBridgeState>) -> Result<()> {
        // Reallocate TempServiceRequestsDataAccount
        let temp_service_requests_usage = ctx.accounts.temp_service_requests_data_account.service_requests.len() * std::mem::size_of::<ServiceRequest>();
        reallocate_account_data(&mut ctx.accounts.temp_service_requests_data_account.to_account_info(), temp_service_requests_usage)?;

        // Reallocate BridgeNodesDataAccount
        let bridge_nodes_usage = ctx.accounts.bridge_nodes_data_account.bridge_nodes.len() * std::mem::size_of::<BridgeNode>();
        reallocate_account_data(&mut ctx.accounts.bridge_nodes_data_account.to_account_info(), bridge_nodes_usage)?;

        // Reallocate ServiceRequestTxidMappingDataAccount
        let txid_mapping_usage = ctx.accounts.service_request_txid_mapping_data_account.mappings.len() * std::mem::size_of::<ServiceRequestTxidMapping>();
        reallocate_account_data(&mut ctx.accounts.service_request_txid_mapping_data_account.to_account_info(), txid_mapping_usage)?;

        // Reallocate AggregatedConsensusDataAccount
        let consensus_data_usage = ctx.accounts.aggregated_consensus_data_account.consensus_data.len() * std::mem::size_of::<AggregatedConsensusData>();
        reallocate_account_data(&mut ctx.accounts.aggregated_consensus_data_account.to_account_info(), consensus_data_usage)?;

        msg!("Reallocation of bridge contract state and related PDAs completed.");
        Ok(())
    }
}

#[account]
pub struct BridgeNodeDataAccount {
    pub bridge_nodes: Vec<BridgeNode>,
}

#[derive(Accounts)]
pub struct RegisterNewBridgeNode<'info> {
    /// CHECK: Manual checks are performed in the instruction to ensure the contributor_account is valid and safe to use.
    #[account(mut, signer)]
    pub user: AccountInfo<'info>, // Ensure this account is a signer and mutable
    
    #[account(mut)]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    #[account(mut)]
    pub bridge_reward_pool_account: Account<'info, BridgeRewardPoolAccount>,

    #[account(mut)]
    pub reg_fee_receiving_account: Account<'info, RegFeeReceivingAccount>,

    pub system_program: Program<'info, System>,
}

pub fn register_new_bridge_node_helper(
    ctx: Context<RegisterNewBridgeNode>,
    pastel_id: String,
    bridge_node_psl_address: String,
) -> Result<()> {
    let bridge_nodes_data_account = &mut ctx.accounts.bridge_nodes_data_account;
    msg!("Initiating new bridge node registration: {}", ctx.accounts.user.key());

    // Check if the bridge node is already registered
    if bridge_nodes_data_account.bridge_nodes.iter().any(|node| node.pastel_id == pastel_id) {
        msg!("Registration failed: Bridge Node already registered with Pastel ID: {}", pastel_id);
        return Err(BridgeError::BridgeNodeAlreadyRegistered.into());
    }

    // Retrieve mutable references to the lamport balance
    let fee_receiving_account_info = ctx.accounts.reg_fee_receiving_account.to_account_info();
    let mut fee_receiving_account_lamports = fee_receiving_account_info.lamports.borrow_mut();

    let reward_pool_account_info = ctx.accounts.bridge_reward_pool_account.to_account_info();
    let mut reward_pool_account_lamports = reward_pool_account_info.lamports.borrow_mut();

    // Check if the reg_fee_receiving_account received the registration fee
    if **fee_receiving_account_lamports < BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS {
        return Err(BridgeError::RegistrationFeeNotPaid.into());
    }

    msg!("Registration fee verified. Attempting to register new bridge node {}", ctx.accounts.user.key());

    // Deduct the registration fee from the reg_fee_receiving_account and add it to the reward pool account
    **fee_receiving_account_lamports -= BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS;
    **reward_pool_account_lamports += BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS;

    let last_active_timestamp = Clock::get()?.unix_timestamp as u64;
    
    // Create and add the new bridge node
    let new_bridge_node = BridgeNode {
        pastel_id: pastel_id.clone(),
        reward_address: *ctx.accounts.user.key,
        bridge_node_psl_address,
        registration_entrance_fee_transaction_signature: String::new(), // Replace with actual data if available
        compliance_score: 1.0, // Initial compliance score
        reliability_score: 1.0, // Initial reliability score
        last_active_timestamp, // Set the last active timestamp to the current time
        total_price_quotes_submitted: 0, // Initially, no price quotes have been submitted
        total_service_requests_attempted: 0, // Initially, no service requests attempted
        successful_service_requests_count: 0, // Initially, no successful service requests
        current_streak: 0, // No streak at the beginning
        failed_service_requests_count: 0, // No failed service requests at the start
        ban_expiry: 0, // No ban initially set
        is_eligible_for_rewards: false, // Initially not eligible for rewards
        is_recently_active: false, // Initially not considered active
        is_reliable: false, // Initially not considered reliable
        is_banned: false, // Initially not banned
    };

    // Append the new bridge node to the BridgeNodesDataAccount
    bridge_nodes_data_account.bridge_nodes.push(new_bridge_node);

    // Logging for debug purposes
    msg!("New Bridge Node successfully Registered: Pastel ID: {}, Timestamp: {}", pastel_id, last_active_timestamp);
    Ok(())
}

// Function to generate the service_request_id based on the service type, first 6 characters of the file hash, and the end user's Solana address from which they will be paying for the service.
pub fn generate_service_request_id(
    pastel_ticket_type_string: &String,
    first_6_chars_of_hash: &String,
    user_sol_address: &Pubkey,
) -> String {
    let user_sol_address_str = user_sol_address.to_string();
    let concatenated_str = format!(
        "{}{}{}",
        pastel_ticket_type_string,
        first_6_chars_of_hash,
        user_sol_address_str,
    );

    // Convert the concatenated string to bytes
    let preimage_bytes = concatenated_str.as_bytes();
    // Compute hash
    let service_request_id_hash = hash(preimage_bytes);
    
    // Convert the first 20 bytes of Hash to a hex string
    let service_request_id_truncated = &service_request_id_hash.to_bytes()[..20];
    let hex_string: String = service_request_id_truncated.iter().map(|byte| format!("{:02x}", byte)).collect();

    // Optional logging
    msg!("Generated truncated service_request_id (hex): {}", hex_string);

    hex_string
}

// Operational accounts for the bridge contract for handling submissions of service requests from end users and price quotes and service request updates from bridge nodes:

#[account]
pub struct ServiceRequestSubmissionAccount {
    pub service_request: ServiceRequest,
}

// Operational PDA to receive service requests from end users
#[derive(Accounts)]
#[instruction(pastel_ticket_type_string: String, first_6_chars_of_hash: String)]
pub struct SubmitServiceRequest<'info> {
    #[account(
        init_if_needed,
        payer = user,
        seeds = [b"svc_request", generate_service_request_id(
            &pastel_ticket_type_string,
            &first_6_chars_of_hash,
            &user.key()
        ).as_bytes()],
        bump,
        space = 8 + std::mem::size_of::<ServiceRequest>()
    )]
    pub service_request_submission_account: Account<'info, ServiceRequestSubmissionAccount>,

    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(mut, seeds = [b"temp_service_requests_data"], bump)]
    pub temp_service_requests_data_account: Account<'info, TempServiceRequestsDataAccount>,

    #[account(mut, seeds = [b"aggregated_consensus_data"], bump)]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,

    pub system_program: Program<'info, System>,
}

pub fn validate_service_request(request: &ServiceRequest) -> Result<()> {
    // Check if the first 6 characters of the SHA3-256 hash are valid
    if request.first_6_characters_of_sha3_256_hash_of_corresponding_file.len() != 6 
        || !request.first_6_characters_of_sha3_256_hash_of_corresponding_file.chars().all(|c| c.is_ascii_hexdigit()) {
        msg!("Error: Invalid or missing first 6 characters of SHA3-256 hash");
        return Err(BridgeError::InvalidFileHash.into());
    }

    // Check if IPFS CID is present
    if request.ipfs_cid.trim().is_empty() {
        msg!("Error: IPFS CID is empty");
        return Err(BridgeError::InvalidIpfsCid.into());
    }

    // Check file size
    if request.file_size_bytes == 0 {
        msg!("Error: File size cannot be zero");
        return Err(BridgeError::InvalidFileSize.into());
    }

    // Check if the user Solana address is valid
    if request.user_sol_address == Pubkey::default() {
        msg!("Error: User Solana address is missing or invalid");
        return Err(BridgeError::InvalidUserSolAddress.into());
    }

    if !matches!(request.service_type, PastelTicketType::Sense | PastelTicketType::Cascade | PastelTicketType::Nft | PastelTicketType::InferenceApi) {
        msg!("Error: Invalid or unsupported service type");
        return Err(BridgeError::InvalidServiceType.into());
    }

    if request.status != RequestStatus::Pending {
        msg!("Error: Initial service request status must be Pending");
        return Err(BridgeError::InvalidRequestStatus.into());
    }

    if request.payment_in_escrow {
        msg!("Error: Payment in escrow should initially be false");
        return Err(BridgeError::InvalidPaymentInEscrow.into());
    }

    if request.selected_bridge_node_pastelid.is_some() || request.best_quoted_price_in_lamports.is_some() {
        msg!("Error: Initial values for certain fields must be None");
        return Err(BridgeError::InvalidInitialFieldValues.into());
    }

    if request.escrow_amount_lamports.is_some() || request.service_fee_retained_by_bridge_contract_lamports.is_some() {
        msg!("Error: Initial escrow amount and fees must be None or zero");
        return Err(BridgeError::InvalidEscrowOrFeeAmounts.into());
    }
    
    Ok(())
}


pub fn submit_service_request_helper(ctx: Context<SubmitServiceRequest>, pastel_ticket_type_string: String, first_6_chars_of_hash: String, ipfs_cid: String, file_size_bytes: u64) -> ProgramResult {
    let current_timestamp = Clock::get()?.unix_timestamp as u64;

    // Convert the pastel_ticket_type_string to PastelTicketType enum
    let service_type: PastelTicketType = match pastel_ticket_type_string.as_str() {
        "Sense" => PastelTicketType::Sense,
        "Cascade" => PastelTicketType::Cascade,
        "Nft" => PastelTicketType::Nft,
        "InferenceApi" => PastelTicketType::InferenceApi,
        _ => return Err(ProgramError::InvalidArgument),
    };

    // Generate the service_request_id
    let service_request_id = generate_service_request_id(
        &pastel_ticket_type_string,
        &first_6_chars_of_hash,
        &ctx.accounts.user.key(),
    );
    
    // Check for duplicate service_request_id
    let temp_service_requests_data_account = &mut ctx.accounts.temp_service_requests_data_account;
    if temp_service_requests_data_account.service_requests.iter().any(|request| request.service_request_id == service_request_id) {
        msg!("Error: Duplicate service request ID submitted");
        return Err(BridgeError::DuplicateServiceRequestId.into());
    }

    // Create or update the ServiceRequest struct
    let service_request_account = &mut ctx.accounts.service_request_submission_account;
    service_request_account.service_request = ServiceRequest {
        service_request_id,
        service_type,
        first_6_characters_of_sha3_256_hash_of_corresponding_file: first_6_chars_of_hash.clone(),
        ipfs_cid,
        file_size_bytes,
        user_sol_address: *ctx.accounts.user.key,
        status: RequestStatus::Pending,
        payment_in_escrow: false,
        request_expiry: current_timestamp + ESCROW_DURATION, // Set appropriate expiry timestamp
        sol_received_from_user_timestamp: None,
        selected_bridge_node_pastelid: None,
        best_quoted_price_in_lamports: None,
        service_request_creation_timestamp: current_timestamp,
        bridge_node_selection_timestamp: None,
        bridge_node_submission_of_txid_timestamp: None,
        submission_of_txid_to_oracle_timestamp: None,
        service_request_completion_timestamp: None,
        payment_received_timestamp: None,
        payment_release_timestamp: None,
        escrow_amount_lamports: None,
        service_fee_retained_by_bridge_contract_lamports: None,
        pastel_txid: None,
    };

    // Call validate_service_request function
    validate_service_request(&service_request_account.service_request)?;        

    // Append the new service request to the temp_service_requests_data_account
    let temp_service_requests_data_account = &mut ctx.accounts.temp_service_requests_data_account;
    temp_service_requests_data_account.service_requests.push(service_request_account.service_request.clone());

    Ok(())
}

#[account]
pub struct BestPriceQuoteReceivedForServiceRequest {
    pub service_request_id: String,
    pub best_bridge_node_pastel_id: String,
    pub best_quoted_price_in_lamports: u64,
    pub best_quote_timestamp: u64,
    pub best_quote_selection_status: BestQuoteSelectionStatus,
}


// Struct to hold a service price quote submission
#[account]
pub struct ServicePriceQuoteSubmissionAccount {
    pub price_quote: ServicePriceQuote,
    pub price_quote_status: ServicePriceQuoteStatus,
}

// PDA to receive price quotes from bridge nodes
#[derive(Accounts)]
#[instruction(service_request_id: String)] // Add this instruction argument
pub struct SubmitPriceQuote<'info> {
    #[account(
        init,
        payer = user,
        seeds = [b"px_quote", service_request_id.as_bytes()], // Use as_bytes() to convert to &[u8]
        bump,
        space = 8 + std::mem::size_of::<ServicePriceQuote>()
    )]
    pub price_quote_submission_account: Account<'info, ServicePriceQuoteSubmissionAccount>,

    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    #[account(mut)]
    pub temp_service_requests_data_account: Account<'info, TempServiceRequestsDataAccount>,

    #[account(mut)]
    pub user: Signer<'info>,

    // System program for account creation
    pub system_program: Program<'info, System>,

    // Bridge Nodes Data Account
    #[account(mut)]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    // Account to store the best price quote for a service request
    #[account(
        mut,
        seeds = [b"bst_px_qt", service_request_id.as_bytes()], // Use as_bytes() here as well
        bump
    )]
    pub best_price_quote_account: Account<'info, BestPriceQuoteReceivedForServiceRequest>,
}


pub fn submit_price_quote_helper(
    ctx: Context<SubmitPriceQuote>,
    bridge_node_pastel_id: String,
    service_request_id: String,
    quoted_price_lamports: u64,
) -> ProgramResult {

    // Obtain the current timestamp
    let quote_timestamp = Clock::get()?.unix_timestamp as u64;

    // Find the service request using the service_request_id_string
    let service_request = ctx.accounts.temp_service_requests_data_account.service_requests.iter()
        .find(|request| request.service_request_id == service_request_id)
        .ok_or(ProgramError::Custom(BridgeError::ServiceRequestNotFound as u32))?;

    // Check if the current timestamp is within the allowed window for quote submission
    if quote_timestamp < service_request.service_request_creation_timestamp + DURATION_IN_SECONDS_TO_WAIT_AFTER_ANNOUNCING_NEW_PENDING_SERVICE_REQUEST_BEFORE_SELECTING_BEST_QUOTE {
        msg!("Quote submission is too early. Please wait until the waiting period is over.");
        return Err(ProgramError::Custom(BridgeError::QuoteResponseTimeExceeded as u32));
    }

    // Validate the price quote
    validate_price_quote_submission(
        &ctx.accounts.bridge_nodes_data_account,
        &ctx.accounts.temp_service_requests_data_account,
        &ctx.accounts.price_quote_submission_account,
        service_request_id.clone(),
        bridge_node_pastel_id,
        quoted_price_lamports,
        quote_timestamp,
    )?;

    // Find the bridge node that is submitting the quote and get a mutable reference
    let submitting_bridge_node = ctx.accounts.bridge_nodes_data_account.bridge_nodes.iter_mut().find(|node| {
        node.reward_address == ctx.accounts.user.key()
    }).ok_or(BridgeError::UnauthorizedBridgeNode)?;

    // Update the total number of price quotes submitted by the bridge node
    submitting_bridge_node.total_price_quotes_submitted += 1;

    // Add the price quote to the price_quote_submission_account
    let price_quote_account = &mut ctx.accounts.price_quote_submission_account;
    price_quote_account.price_quote = ServicePriceQuote {
        service_request_id: service_request_id.clone(),
        bridge_node_pastel_id: submitting_bridge_node.pastel_id.clone(),
        quoted_price_lamports,
        quote_timestamp, // Use internally generated timestamp
        price_quote_status: ServicePriceQuoteStatus::Submitted,
    };

    // Update best price quote if the new quote is better
    let best_quote_account = &mut ctx.accounts.best_price_quote_account;
    if best_quote_account.best_quote_selection_status == BestQuoteSelectionStatus::NoQuotesReceivedYet ||
    quoted_price_lamports < best_quote_account.best_quoted_price_in_lamports {
        best_quote_account.best_bridge_node_pastel_id = submitting_bridge_node.pastel_id.clone();
        best_quote_account.best_quoted_price_in_lamports = quoted_price_lamports;
        best_quote_account.best_quote_timestamp = quote_timestamp;
        best_quote_account.best_quote_selection_status = BestQuoteSelectionStatus::BestQuoteSelected;
    }

    Ok(())
}


pub fn validate_price_quote_submission(
    bridge_nodes_data_account: &BridgeNodesDataAccount,
    temp_service_requests_data_account: &TempServiceRequestsDataAccount,
    price_quote_submission_account: &ServicePriceQuoteSubmissionAccount,
    service_request_id: String,
    bridge_node_pastel_id: String,
    quoted_price_lamports: u64,
    quote_timestamp: u64,
) -> Result<()> {

    // Check if the service_request_id corresponds to an existing and pending service request
    let service_request = temp_service_requests_data_account
        .service_requests
        .iter()
        .find(|request| request.service_request_id == *service_request_id)
        .ok_or(BridgeError::ServiceRequestNotFound)?;

    // Validate service request status
    if service_request.status != RequestStatus::Pending {
        msg!("Error: Service request is not in a pending state");
        return Err(BridgeError::ServiceRequestNotPending.into());
    }

    // Ensure the price quote is submitted within MAX_QUOTE_RESPONSE_TIME seconds of service request creation
    if quote_timestamp > service_request.service_request_creation_timestamp + MAX_QUOTE_RESPONSE_TIME {
        msg!("Error: Price quote submitted beyond the maximum response time");
        return Err(BridgeError::QuoteResponseTimeExceeded.into());
    }

    // Check Maximum Quote Response Time
    let current_timestamp = Clock::get()?.unix_timestamp as u64;
    if quote_timestamp > service_request.service_request_creation_timestamp + MAX_DURATION_IN_SECONDS_FROM_LAST_REPORT_SUBMISSION_BEFORE_SELECTING_WINNING_QUOTE {
        msg!("Error: Price quote submitted too late");
        return Err(BridgeError::QuoteResponseTimeExceeded.into());
    }

    // Check if the bridge node is registered
    let registered_bridge_node = bridge_nodes_data_account
        .bridge_nodes
        .iter()
        .find(|node| &node.pastel_id == &bridge_node_pastel_id);

    if let Some(bridge_node) = registered_bridge_node {
        // Check if the bridge node is banned
        if bridge_node.calculate_is_banned(current_timestamp) {
            msg!("Error: Bridge node is currently banned");
            return Err(BridgeError::BridgeNodeBanned.into());
        }
    } else {
        msg!("Error: Bridge node is not registered");
        return Err(BridgeError::UnregisteredBridgeNode.into());
    }

    // Check if the quoted price in lamports is not zero
    if quoted_price_lamports == 0 {
        msg!("Error: Quoted price cannot be zero");
        return Err(BridgeError::InvalidQuotedPrice.into());
    }

    // Check if the price quote status is set to 'Submitted'
    if price_quote_submission_account.price_quote_status != ServicePriceQuoteStatus::Submitted {
        msg!("Error: Price quote status is not set to 'Submitted'");
        return Err(BridgeError::InvalidQuoteStatus.into());
    }

    // Time Validity of the Quote
    let current_timestamp = Clock::get()?.unix_timestamp as u64;
    if current_timestamp > price_quote_submission_account.price_quote.quote_timestamp + QUOTE_VALIDITY_DURATION {
        msg!("Error: Price quote has expired");
        return Err(BridgeError::QuoteExpired.into());
    }

    // Bridge Node Compliance and Reliability Scores for Reward
    if let Some(bridge_node) = registered_bridge_node {
        if bridge_node.compliance_score < MIN_COMPLIANCE_SCORE_FOR_REWARD || bridge_node.reliability_score < MIN_RELIABILITY_SCORE_FOR_REWARD {
            msg!("Error: Bridge node does not meet the minimum score requirements for rewards");
            return Err(BridgeError::BridgeNodeScoreTooLow.into());
        }
        // Bridge Node Activity Status
        if current_timestamp > bridge_node.last_active_timestamp + BRIDGE_NODE_INACTIVITY_THRESHOLD {
            msg!("Error: Bridge node is inactive");
            return Err(BridgeError::BridgeNodeInactive.into());
        }
    }
    Ok(())
}


pub fn choose_best_price_quote(
    best_quote_account: &BestPriceQuoteReceivedForServiceRequest,
    temp_service_requests_data_account: &mut TempServiceRequestsDataAccount,
    current_timestamp: u64
) -> Result<()> {
    // Find the service request corresponding to the best price quote
    let service_request = temp_service_requests_data_account.service_requests
        .iter_mut()
        .find(|request| request.service_request_id == best_quote_account.service_request_id)
        .ok_or(BridgeError::ServiceRequestNotFound)?;

    // Check if the quote is still valid
    if current_timestamp > best_quote_account.best_quote_timestamp + QUOTE_VALIDITY_DURATION {
        return Err(BridgeError::QuoteExpired.into());
    }

    // Update the service request with the selected bridge node details
    service_request.selected_bridge_node_pastelid = Some(best_quote_account.best_bridge_node_pastel_id.clone());
    service_request.best_quoted_price_in_lamports = Some(best_quote_account.best_quoted_price_in_lamports);
    service_request.bridge_node_selection_timestamp = Some(current_timestamp);

    // Change the status of the service request to indicate that a bridge node has been selected
    service_request.status = RequestStatus::BridgeNodeSelected;

    Ok(())
}


fn update_txid_mapping(
    txid_mapping_data_account: &mut Account<ServiceRequestTxidMappingDataAccount>, 
    service_request_id: String, 
    pastel_txid: String
) -> ProgramResult {
    // Check if a mapping for the service_request_id already exists
    if let Some(mapping) = txid_mapping_data_account.mappings.iter_mut().find(|m| m.service_request_id == service_request_id) {
        mapping.pastel_txid = pastel_txid;
    } else {
        // Create a new mapping
        txid_mapping_data_account.mappings.push(ServiceRequestTxidMapping {
            service_request_id,
            pastel_txid,
        });
    }
    
    Ok(())
}

#[derive(Accounts)]
pub struct SubmitPastelTxid<'info> {
    #[account(
        mut,
        constraint = service_request_submission_account.service_request.selected_bridge_node_pastelid.is_some() @ BridgeError::BridgeNodeNotSelected,
        constraint = service_request_submission_account.service_request.status == RequestStatus::Pending @ BridgeError::InvalidRequestStatus,
    )]
    pub service_request_submission_account: Account<'info, ServiceRequestSubmissionAccount>,

    // The bridge contract state
    #[account(
        mut,
        constraint = bridge_contract_state.is_initialized @ BridgeError::ContractNotInitialized
    )]
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    // Bridge Nodes Data Account
    #[account(
        mut,
    )]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    // Account for the ServiceRequestTxidMappingDataAccount PDA
    #[account(
        mut,
        seeds = [b"service_request_txid_map"],
        bump
    )]
    pub service_request_txid_mapping_data_account: Account<'info, ServiceRequestTxidMappingDataAccount>,

    // System program
    pub system_program: Program<'info, System>,
}

pub fn submit_pastel_txid_from_bridge_node_helper(ctx: Context<SubmitPastelTxid>, service_request_id: String, pastel_txid: String) -> ProgramResult {
    let service_request = &mut ctx.accounts.service_request_submission_account.service_request;

    // Validate that the service_request_id matches
    if service_request.service_request_id != service_request_id {
        return Err(BridgeError::InvalidServiceRequestId.into());
    }

    // Validate the format and length of the Pastel TxID
    if pastel_txid.is_empty() || pastel_txid.len() > MAX_TXID_LENGTH {
        return Err(BridgeError::InvalidPastelTxid.into());
    }

    // Find the bridge node that matches the selected_pastel_id
    let selected_bridge_node = ctx.accounts.bridge_nodes_data_account.bridge_nodes.iter().find(|node| {
        node.pastel_id == *service_request.selected_bridge_node_pastelid.as_ref().unwrap()
    }).ok_or(BridgeError::UnregisteredBridgeNode)?;

    // Check if the transaction is being submitted by the selected bridge node
    if selected_bridge_node.reward_address != ctx.accounts.system_program.key() {
        return Err(BridgeError::UnauthorizedBridgeNode.into());
    }

    // Update the service request with the provided Pastel TxID
    service_request.pastel_txid = Some(pastel_txid.clone());
    service_request.bridge_node_submission_of_txid_timestamp = Some(Clock::get()?.unix_timestamp as u64);

    // Change the status of the service request to indicate that the TxID has been submitted
    service_request.status = RequestStatus::AwaitingCompletionConfirmation;

    // Log the TxID submission
    msg!("Pastel TxID submitted by Bridge Node {} for Service Request ID: {}", selected_bridge_node.pastel_id, service_request_id.to_string());

    // Update the TxID mapping for the service request
    let txid_mapping_data_account = &mut ctx.accounts.service_request_txid_mapping_data_account;
    update_txid_mapping(txid_mapping_data_account, service_request_id, pastel_txid.clone())?;

    Ok(())
}

#[derive(Accounts)]
pub struct AccessOracleData<'info> {
    #[account(
        seeds = [b"aggregated_consensus_data"],
        bump,
        constraint = aggregated_consensus_data_account.to_account_info().owner == &bridge_contract_state.oracle_contract_pubkey
    )]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,

    #[account(
        seeds = [b"service_request_txid_map"],
        bump
    )]
    pub service_request_txid_mapping_data_account: Account<'info, ServiceRequestTxidMappingDataAccount>,

    #[account(mut)]
    pub temp_service_requests_data_account: Account<'info, TempServiceRequestsDataAccount>,

    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    #[account(mut)]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    #[account(mut)]
    pub bridge_reward_pool_account: Account<'info, BridgeRewardPoolAccount>,

    #[account(mut)]
    pub bridge_escrow_account: Account<'info, BridgeEscrowAccount>,

    /// CHECK: This account is provided by the user and is manually checked in the program logic to ensure it matches the user who created the service request.
    #[account(mut)]
    pub user_account: AccountInfo<'info>,

    pub system_program: Program<'info, System>,
}

pub fn compute_consensus(aggregated_data: &AggregatedConsensusData) -> (TxidStatus, String) {
    let consensus_status = aggregated_data.status_weights.iter().enumerate().max_by_key(|&(_, weight)| weight)
        .map(|(index, _)| usize_to_txid_status(index).unwrap_or(TxidStatus::Invalid)).unwrap();

    let consensus_hash = aggregated_data.hash_weights.iter().max_by_key(|hash_weight| hash_weight.weight)
        .map(|hash_weight| hash_weight.hash.clone()).unwrap_or_default();

    (consensus_status, consensus_hash)
}

pub fn get_aggregated_data<'a>(
    aggregated_data_account: &'a Account<AggregatedConsensusDataAccount>,
    txid: &str
) -> Option<&'a AggregatedConsensusData> {
    aggregated_data_account.consensus_data.iter()
        .find(|data| data.txid == txid)
}

impl<'info> AccessOracleData<'info> {
    pub fn process(
        &mut self, 
        txid: String, 
        service_request_id: &String,
        bridge_escrow_account: &mut AccountInfo<'info>,
        system_program: &Program<'info, System>,
        service_request_txid_mapping_account: &Account<ServiceRequestTxidMappingDataAccount>,
    ) -> Result<()> {
        let current_timestamp = Clock::get()?.unix_timestamp as u64;

        let service_request = self.temp_service_requests_data_account.service_requests.iter()
            .find(|request| &request.service_request_id == service_request_id)
            .ok_or(BridgeError::ServiceRequestNotFound)?;

        // Validate user account
        if service_request.user_sol_address != *self.user_account.key {
            return Err(BridgeError::InvalidUserSolAddress.into());
        }

        let service_request_data = (
            service_request.user_sol_address,
            service_request.escrow_amount_lamports,
        );
                    
        if current_timestamp > service_request.request_expiry {
            let user_sol_address = self.get_user_sol_address(service_request_id)?;
            send_sol(
                bridge_escrow_account,
                user_sol_address,
                service_request.escrow_amount_lamports.unwrap_or_default(),
                system_program,
            )?;
            self.update_service_request_state(service_request_id, RequestStatus::Failed)?;
            return Ok(());
        }

        // Fetch the aggregated data and drop the borrow immediately
        let consensus_data = {
            let consensus_data_option = get_aggregated_data(&self.aggregated_consensus_data_account, &txid);
            consensus_data_option.cloned() // Clone the data to drop the borrow
        };

        // Extract the necessary data before the if-else block
        let bridge_reward_pool_account_key = self.bridge_reward_pool_account.to_account_info().key();

        if let Some(consensus_data) = consensus_data {
            self.validate_consensus_data(&consensus_data, service_request_id, service_request_txid_mapping_account)?;
            let txid_mined_and_matched = self.is_txid_mined_and_file_hash_matches(&consensus_data, &self.service_request_txid_mapping_data_account, service_request_id);

            if txid_mined_and_matched {
                self.handle_escrow_transactions(
                    &consensus_data,
                    bridge_escrow_account,
                    bridge_reward_pool_account_key,
                    true, // success
                    service_request_id,
                    system_program,
                    service_request_data,
                )?;
            } else {
                self.handle_escrow_transactions(
                    &consensus_data,
                    bridge_escrow_account,
                    bridge_reward_pool_account_key,
                    false, // failure (refund)
                    service_request_id,
                    system_program,
                    service_request_data,
                )?;
            }
        } else {
            return Err(BridgeError::TxidNotFound.into());
        }

        Ok(())
    }

    fn get_selected_bridge_node_sol_address(&self, service_request_id: &String) -> Result<Pubkey> {
        let bridge_node_pastelid = self.temp_service_requests_data_account.service_requests.iter()
            .find(|request| &request.service_request_id == service_request_id)
            .and_then(|request| request.selected_bridge_node_pastelid.as_ref())
            .ok_or(BridgeError::BridgeNodeNotSelected)?;
    
        self.bridge_nodes_data_account.bridge_nodes.iter()
            .find(|node| &node.pastel_id == bridge_node_pastelid)
            .map(|node| node.reward_address)
            .ok_or(BridgeError::BridgeNodeNotSelected.into())
    }

    fn get_user_sol_address(&self, service_request_id: &String) -> Result<Pubkey> {
        self.temp_service_requests_data_account.service_requests.iter()
            .find(|request| &request.service_request_id == service_request_id)
            .map(|request| request.user_sol_address)
            .ok_or(BridgeError::ServiceRequestNotFound.into())
    }

    fn validate_consensus_data(
        &self,
        consensus_data: &AggregatedConsensusData,
        service_request_id: &String,
        service_request_txid_mapping_account: &Account<ServiceRequestTxidMappingDataAccount>
    ) -> Result<()> {
        // Validate the txid
        let mapping = service_request_txid_mapping_account.mappings
            .iter()
            .find(|mapping| mapping.service_request_id == *service_request_id)
            .ok_or(BridgeError::MappingNotFound)?;
        if mapping.pastel_txid != consensus_data.txid {
            return Err(BridgeError::TxidMismatch.into());
        }

        // Check if the last_updated timestamp is recent enough
        let current_timestamp = Clock::get()?.unix_timestamp as u64;
        if current_timestamp > consensus_data.last_updated + MAX_ALLOWED_TIMESTAMP_DIFFERENCE {
            return Err(BridgeError::OutdatedConsensusData.into());
        }

        Ok(())
    }

    fn is_txid_mined_and_file_hash_matches(
        &self, 
        consensus_data: &AggregatedConsensusData, 
        txid_mappings: &Account<ServiceRequestTxidMappingDataAccount>,
        service_request_id: &String
    ) -> bool {
        let (consensus_status, consensus_hash) = compute_consensus(consensus_data);
    
        if consensus_status != TxidStatus::MinedActivated {
            return false;
        }
    
        // Find the service request and check if the txid matches
        if let Some(mapping) = txid_mappings.mappings.iter().find(|m| &m.service_request_id == service_request_id) {
            if mapping.pastel_txid == consensus_data.txid {
                // Here, we assume that consensus_data contains the aggregated data for the txid in the mapping
                return consensus_data.first_6_characters_of_sha3_256_hash_of_corresponding_file == consensus_hash;
            }
        }
    
        false
    }

    fn update_service_request_state(
        &mut self, 
        service_request_id: &String, 
        status: RequestStatus
    ) -> Result<()> {
        let service_request = self.temp_service_requests_data_account.service_requests.iter_mut()
            .find(|request| request.service_request_id == *service_request_id)
            .ok_or(BridgeError::ServiceRequestNotFound)?;

        service_request.status = status;

        Ok(())
    }    
    
    fn update_service_request_state_to_completed(&mut self, consensus_data: &AggregatedConsensusData) -> Result<()> {
        // Retrieve the mapping between service_request_id and pastel_txid
        let service_request_id = self.get_service_request_id_for_pastel_txid(consensus_data.txid.as_str())?;

        // Find the service request by ID and update its status
        let service_request = self.temp_service_requests_data_account.service_requests.iter_mut()
            .find(|request| request.service_request_id == service_request_id)
            .ok_or(BridgeError::ServiceRequestNotFound)?;

        // Update the status to Completed
        service_request.status = RequestStatus::Completed;
        service_request.service_request_completion_timestamp = Some(
            Clock::get()?
                .unix_timestamp
                .try_into()
                .map_err(|_| BridgeError::TimestampConversionError)?
        );
        
        Ok(())
    }

    fn get_service_request_id_for_pastel_txid(&self, txid: &str) -> Result<String> {
        self.service_request_txid_mapping_data_account.mappings.iter()
            .find(|mapping| mapping.pastel_txid == txid)
            .map(|mapping| mapping.service_request_id.clone()) // Clone the string
            .ok_or(BridgeError::TxidMappingNotFound.into())
    }

    fn handle_escrow_transactions(
        &mut self,
        consensus_data: &AggregatedConsensusData,
        bridge_escrow_account: &mut AccountInfo<'info>,
        bridge_reward_pool_account_key: Pubkey,
        is_success: bool,
        service_request_id: &String,
        system_program: &Program<'info, System>,
        service_request_data: (Pubkey, Option<u64>),
    ) -> Result<()> {
        let (user_sol_address, escrow_amount_lamports) = service_request_data;
        let escrow_amount = escrow_amount_lamports.ok_or(BridgeError::EscrowNotFunded)?;
    
        if is_success {
            // Calculate the service fee
            let service_fee = escrow_amount * TRANSACTION_FEE_PERCENTAGE as u64 / 100;
            let amount_to_bridge_node = escrow_amount - service_fee;
    
            // Transfer the service fee to the bridge reward pool account
            send_sol(
                bridge_escrow_account,
                bridge_reward_pool_account_key,
                service_fee,
                system_program,
            )?;
    
            // Send the remaining amount to the bridge node
            send_sol(
                bridge_escrow_account,
                self.get_selected_bridge_node_sol_address(service_request_id)?,
                amount_to_bridge_node,
                system_program,
            )?;
    
            // Update the service request state to Completed
            self.update_service_request_state_to_completed(consensus_data)?;
    
            msg!("Service Request ID: {} completed successfully! Sent {} SOL to Bridge Node at address: {} and transferred the service fee of {} SOL to the Bridge Reward Pool",
                service_request_id,
                amount_to_bridge_node as f64 / LAMPORTS_PER_SOL as f64,
                self.get_selected_bridge_node_sol_address(service_request_id)?,
                service_fee as f64 / LAMPORTS_PER_SOL as f64,
            );

        } else {
            // Refund the full escrow amount to the user in case of failure
            send_sol(
                bridge_escrow_account,
                user_sol_address,
                escrow_amount,
                system_program,
            )?;
    
            // Update the service request state to Failed
            self.update_service_request_state(service_request_id, RequestStatus::Failed)?;
        }
    
        Ok(())
    }

}


// Refactored to be standalone functions
pub fn send_sol<'info>(
    from_account: &mut AccountInfo<'info>, 
    to_pubkey: Pubkey, 
    amount: u64,
    system_program: &Program<'info, System>,
) -> ProgramResult {
    let ix = system_instruction::transfer(
        from_account.key, 
        &to_pubkey, 
        amount,
    );

    invoke(
        &ix,
        &[
            from_account.clone(),
            system_program.to_account_info(),
        ],
    )
}

pub fn refund_escrow_to_user(
    escrow_account: &mut AccountInfo<'_>, 
    user_account: &mut AccountInfo<'_>, 
    refund_amount: u64
) -> ProgramResult {
    // Check if escrow account has enough balance
    if **escrow_account.lamports.borrow() < refund_amount {
        return Err(BridgeError::InsufficientEscrowFunds.into());
    }

    // Transfer the refund from the escrow account to the user account
    **escrow_account.try_borrow_mut_lamports()? -= refund_amount;
    **user_account.try_borrow_mut_lamports()? += refund_amount;

    Ok(())
}

pub fn apply_bans(bridge_node: &mut BridgeNode, current_timestamp: u64, success: bool) {
    if !success {
        bridge_node.failed_service_requests_count += 1;

        if bridge_node.total_service_requests_attempted <= SERVICE_REQUESTS_FOR_TEMPORARY_BAN && bridge_node.failed_service_requests_count >= TEMPORARY_BAN_SERVICE_FAILURES_THRESHOLD {
            bridge_node.ban_expiry = current_timestamp + TEMPORARY_BAN_DURATION;
            msg!("Bridge Node: {} temporarily banned due to failed service requests", bridge_node.pastel_id);
        } else if bridge_node.total_service_requests_attempted >= SERVICE_REQUESTS_FOR_PERMANENT_BAN {
            bridge_node.ban_expiry = u64::MAX;
            msg!("Bridge Node: {} permanently banned due to total service requests", bridge_node.pastel_id);
        }
    } else {
        bridge_node.failed_service_requests_count = 0;
    }
}

pub fn update_scores(bridge_node: &mut BridgeNode, current_timestamp: u64, success: bool) {
    let time_diff = current_timestamp.saturating_sub(bridge_node.last_active_timestamp);
    let hours_inactive: f32 = time_diff as f32 / 3_600.0;

    let success_scaling = if success { (1.0 + bridge_node.current_streak as f32 * 0.1).min(2.0) } else { 1.0 };
    let time_weight = 1.0 / (1.0 + hours_inactive / 480.0);
    let score_increment = 20.0 * success_scaling * time_weight;
    let score_decrement = 20.0 * (1.0 + bridge_node.failed_service_requests_count as f32 * 0.5).min(3.0);
    let decay_rate: f32 = 0.99;
    let decay_factor = decay_rate.powf(hours_inactive / 24.0);

    let streak_bonus = if success { (bridge_node.current_streak as f32 / 10.0).min(3.0).max(0.0) } else { 0.0 };

    if success {
        bridge_node.successful_service_requests_count += 1;
        bridge_node.current_streak += 1;
        bridge_node.compliance_score += score_increment + streak_bonus;
    } else {
        bridge_node.current_streak = 0;
        bridge_node.compliance_score = (bridge_node.compliance_score - score_decrement).max(0.0);
    }

    bridge_node.compliance_score *= decay_factor;

    let reliability_factor = (bridge_node.successful_service_requests_count as f32 / bridge_node.total_service_requests_attempted as f32).clamp(0.0, 1.0);
    bridge_node.compliance_score = (bridge_node.compliance_score * reliability_factor).min(100.0);
    bridge_node.compliance_score = logistic_scale(bridge_node.compliance_score, 100.0, 0.1, 50.0);
    bridge_node.reliability_score = reliability_factor * 100.0;

    if bridge_node.compliance_score < MIN_COMPLIANCE_SCORE_FOR_REWARD || bridge_node.reliability_score < MIN_RELIABILITY_SCORE_FOR_REWARD {
        bridge_node.is_eligible_for_rewards = false;
    } else {
        bridge_node.is_eligible_for_rewards = true;
    }

    log_score_updates(bridge_node);
}

pub fn logistic_scale(score: f32, max_value: f32, steepness: f32, midpoint: f32) -> f32 {
    max_value / (1.0 + (-steepness * (score - midpoint)).exp())
}

pub fn log_score_updates(bridge_node: &BridgeNode) {
    msg!("Scores After Update: Pastel ID: {}, Compliance Score: {}, Reliability Score: {}",
        bridge_node.pastel_id, bridge_node.compliance_score, bridge_node.reliability_score);
}

pub fn update_bridge_node_status(
    bridge_node: &mut BridgeNode,
    success: bool,
    current_timestamp: u64
) {
    bridge_node.total_service_requests_attempted += 1;
    bridge_node.last_active_timestamp = current_timestamp;

    if success {
        bridge_node.successful_service_requests_count += 1;
        bridge_node.current_streak += 1;

        // Determine eligibility for rewards based on compliance and reliability scores
        bridge_node.is_eligible_for_rewards = bridge_node.compliance_score >= MIN_COMPLIANCE_SCORE_FOR_REWARD
                                            && bridge_node.reliability_score >= MIN_RELIABILITY_SCORE_FOR_REWARD;
    } else {
        bridge_node.failed_service_requests_count += 1;
        bridge_node.current_streak = 0;

        // Apply a temporary ban if the node has attempted enough service requests but has a high failure rate
        if bridge_node.total_service_requests_attempted > SERVICE_REQUESTS_FOR_TEMPORARY_BAN 
        && bridge_node.failed_service_requests_count > TEMPORARY_BAN_SERVICE_FAILURES_THRESHOLD {
            bridge_node.ban_expiry = current_timestamp + TEMPORARY_BAN_DURATION;
        }
    }

    update_scores(bridge_node, current_timestamp, success);
    update_statuses(bridge_node, current_timestamp);
}


pub fn update_statuses(bridge_node: &mut BridgeNode, current_timestamp: u64) {
    // Update the recently active status based on the last active timestamp and the inactivity threshold
    bridge_node.is_recently_active = current_timestamp - bridge_node.last_active_timestamp < BRIDGE_NODE_INACTIVITY_THRESHOLD;

    // Update the reliability status based on the ratio of successful to attempted service requests
    bridge_node.is_reliable = if bridge_node.total_service_requests_attempted > 0 {
        let success_ratio = bridge_node.successful_service_requests_count as f32 
                            / bridge_node.total_service_requests_attempted as f32;
        success_ratio >= MIN_RELIABILITY_SCORE_FOR_REWARD / 100.0
    } else {
        false
    };

    // Update the eligibility for rewards based on compliance and reliability scores
    bridge_node.is_eligible_for_rewards = bridge_node.compliance_score >= MIN_COMPLIANCE_SCORE_FOR_REWARD 
                                        && bridge_node.reliability_score >= MIN_RELIABILITY_SCORE_FOR_REWARD;
}


pub fn apply_permanent_bans(bridge_nodes_data_account: &mut Account<BridgeNodesDataAccount>) {
    // Collect Pastel IDs of bridge nodes to be removed for efficient logging
    let bridge_nodes_to_remove: Vec<String> = bridge_nodes_data_account.bridge_nodes.iter()
        .filter(|node| node.ban_expiry == u64::MAX)
        .map(|node| node.pastel_id.clone()) // Clone the Pastel ID
        .collect();

    // Log information about the removal process
    msg!("Now removing permanently banned bridge nodes! Total number of bridge nodes before removal: {}, Number of bridge nodes to be removed: {}, Pastel IDs of bridge nodes to be removed: {:?}",
        bridge_nodes_data_account.bridge_nodes.len(), bridge_nodes_to_remove.len(), bridge_nodes_to_remove);

    // Retain only bridge nodes who are not permanently banned
    bridge_nodes_data_account.bridge_nodes.retain(|node| node.ban_expiry != u64::MAX);
}


pub fn handle_post_transaction_tasks(
    bridge_nodes_data_account: &mut Account<BridgeNodesDataAccount>,
    temp_service_requests_data_account: &mut Account<TempServiceRequestsDataAccount>,
    service_request_txid_mapping_data_account: &mut Account<ServiceRequestTxidMappingDataAccount>,
    current_timestamp: u64,
) -> Result<()> {
    apply_permanent_bans(bridge_nodes_data_account);

    msg!("Now cleaning up unneeded data in TempServiceRequestsDataAccount...");
    // Cleanup unneeded data in TempServiceRequestsDataAccount
    temp_service_requests_data_account.service_requests.retain(|service_request| {
        current_timestamp - service_request.service_request_creation_timestamp < SERVICE_REQUEST_VALIDITY
    });

    msg!("Now cleaning up unneeded data in ServiceRequestTxidMappingDataAccount...");
    // Cleanup old mapping data in ServiceRequestTxidMappingDataAccount
    service_request_txid_mapping_data_account.mappings.retain(|mapping| {
        // Find the corresponding service request for this mapping
        if let Some(service_request) = temp_service_requests_data_account.service_requests.iter().find(|sr| sr.service_request_id == mapping.service_request_id) {
            // Retain the mapping if it's within the validity period
            current_timestamp - service_request.service_request_creation_timestamp < DATA_RETENTION_PERIOD
        } else {
            // If there is no corresponding service request, do not retain the mapping
            false
        }
    });

    msg!("Done with post-consensus tasks!");
    Ok(())
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct HashWeight {
    pub hash: String,
    pub weight: i32,
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

impl BridgeNode {

    // Check if the contributor is currently banned
    pub fn calculate_is_banned(&self, current_time: u64) -> bool {
        current_time < self.ban_expiry
    }

    // Method to determine if the contributor is eligible for rewards
    pub fn calculate_is_eligible_for_rewards(&self) -> bool {
        self.reliability_score >= MIN_RELIABILITY_SCORE_FOR_REWARD && self.compliance_score >= MIN_COMPLIANCE_SCORE_FOR_REWARD
    }

}


#[derive(Accounts)]
pub struct SetOracleContract<'info> {
    #[account(mut, has_one = admin_pubkey)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,
    pub admin_pubkey: Signer<'info>,
}

impl<'info> SetOracleContract<'info> {
    pub fn set_oracle_contract(ctx: Context<SetOracleContract>, oracle_contract_pubkey: Pubkey) -> Result<()> {
        let state = &mut ctx.accounts.bridge_contract_state;
        state.oracle_contract_pubkey = oracle_contract_pubkey;
        msg!("Oracle contract pubkey updated: {:?}", oracle_contract_pubkey);
        Ok(())
    }
}


#[derive(Accounts)]
pub struct WithdrawFunds<'info> {
    #[account(
        mut,
        constraint = bridge_contract_state.admin_pubkey == *admin_account.key @ BridgeError::UnauthorizedWithdrawalAccount,
    )]
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    /// CHECK: The admin_account is manually verified in the instruction to ensure it's the correct and authorized account for withdrawal operations. This includes checking if the account matches the admin_pubkey stored in bridge_contract_state.
    pub admin_account: AccountInfo<'info>,

    #[account(mut)]
    pub bridge_reward_pool_account: Account<'info, BridgeRewardPoolAccount>,

    #[account(mut)]
    pub bridge_escrow_account: Account<'info, BridgeEscrowAccount>,

    #[account(mut)]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    #[account(mut)]
    pub temp_service_requests_data_account: Account<'info, TempServiceRequestsDataAccount>,

    #[account(mut)]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,

    #[account(mut)]
    pub service_request_txid_mapping_data_account: Account<'info, ServiceRequestTxidMappingDataAccount>,

    pub system_program: Program<'info, System>,
}

impl<'info> WithdrawFunds<'info> {
    pub fn execute(ctx: Context<WithdrawFunds>, 
                reward_pool_amount: u64, 
                fee_receiving_amount: u64,
                escrow_amount: u64,
                bridge_nodes_data_amount: u64,
                temp_service_requests_data_amount: u64,
                aggregated_consensus_data_amount: u64,
                service_request_txid_mapping_data_amount: u64) -> Result<()> {
        if !ctx.accounts.admin_account.is_signer {
            return Err(BridgeError::UnauthorizedWithdrawalAccount.into()); // Check if the admin_account is a signer
        }

        let admin_account = &mut ctx.accounts.admin_account;
        let bridge_reward_pool_account = &mut ctx.accounts.bridge_reward_pool_account;
        let bridge_escrow_account = &mut ctx.accounts.bridge_escrow_account;
        let bridge_nodes_data_account = &mut ctx.accounts.bridge_nodes_data_account;
        let temp_service_requests_data_account = &mut ctx.accounts.temp_service_requests_data_account;
        let aggregated_consensus_data_account = &mut ctx.accounts.aggregated_consensus_data_account;
        let service_request_txid_mapping_data_account = &mut ctx.accounts.service_request_txid_mapping_data_account;

        // Function to withdraw funds from a specific account
        fn withdraw_funds(from_account: &mut AccountInfo, to_account: &mut AccountInfo, amount: u64) -> Result<()> {
            if **from_account.lamports.borrow() < amount {
                return Err(BridgeError::InsufficientFunds.into());
            }
            **from_account.lamports.borrow_mut() -= amount;
            **to_account.lamports.borrow_mut() += amount;
            Ok(())
        }

        // Perform withdrawals
        withdraw_funds(&mut bridge_reward_pool_account.to_account_info(), admin_account, reward_pool_amount)?;
        withdraw_funds(&mut bridge_escrow_account.to_account_info(), admin_account, escrow_amount)?;
        withdraw_funds(&mut bridge_nodes_data_account.to_account_info(), admin_account, bridge_nodes_data_amount)?;
        withdraw_funds(&mut temp_service_requests_data_account.to_account_info(), admin_account, temp_service_requests_data_amount)?;
        withdraw_funds(&mut aggregated_consensus_data_account.to_account_info(), admin_account, aggregated_consensus_data_amount)?;
        withdraw_funds(&mut service_request_txid_mapping_data_account.to_account_info(), admin_account, service_request_txid_mapping_data_amount)?;

        msg!("Withdrawal successful: {} lamports transferred from various PDAs to admin account", reward_pool_amount + fee_receiving_amount + escrow_amount + bridge_nodes_data_amount + temp_service_requests_data_amount + aggregated_consensus_data_amount + service_request_txid_mapping_data_amount);
        Ok(())
    }
}

declare_id!("Ew8ohkPJ3JnWoZ3MWvkn86wYMRJkS385Bsis9TwQJo79");
#[program]
pub mod solana_pastel_bridge_program {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, admin_pubkey: Pubkey) -> Result<()> {
        msg!("Initializing Bridge Contract State");

        // Call the initialize_bridge_state method from the Initialize implementation
        ctx.accounts.initialize_bridge_state(admin_pubkey)?;

        msg!("Bridge Contract State Initialized with Admin Pubkey: {:?}", admin_pubkey);

        // Logging PDAs for confirmation
        msg!("Bridge Reward Pool Account PDA: {:?}", ctx.accounts.bridge_reward_pool_account.key());
        msg!("Bridge Escrow Account PDA: {:?}", ctx.accounts.bridge_escrow_account.key());
        msg!("Bridge Nodes Data Account PDA: {:?}", ctx.accounts.bridge_nodes_data_account.key());
        msg!("Temporary Service Requests Data Account PDA: {:?}", ctx.accounts.temp_service_requests_data_account.key());
        msg!("Aggregated Consensus Data Account PDA: {:?}", ctx.accounts.aggregated_consensus_data_account.key());
        msg!("Service Request Txid Mapping Data Account PDA: {:?}", ctx.accounts.service_request_txid_mapping_data_account.key());

        Ok(())
    }

    pub fn reallocate_bridge_state(ctx: Context<ReallocateBridgeState>) -> Result<()> {
        ReallocateBridgeState::execute(ctx)
    }

    pub fn register_new_bridge_node(
        ctx: Context<RegisterNewBridgeNode>,
        pastel_id: String,
        bridge_node_psl_address: String,
    ) -> Result<()> {
        // Call the helper function
        register_new_bridge_node_helper(ctx, pastel_id, bridge_node_psl_address)
    }

    pub fn submit_service_request(
        ctx: Context<SubmitServiceRequest>,
        pastel_ticket_type_string: String,
        first_6_chars_of_hash: String,
        ipfs_cid: String,
        file_size_bytes: u64,
    ) -> ProgramResult {
        submit_service_request_helper(ctx, pastel_ticket_type_string, first_6_chars_of_hash, ipfs_cid, file_size_bytes)
    }

    pub fn submit_price_quote(
        ctx: Context<SubmitPriceQuote>,
        bridge_node_pastel_id: String,
        service_request_id_string: String,
        quoted_price_lamports: u64,
    ) -> ProgramResult {
        submit_price_quote_helper(
            ctx,
            bridge_node_pastel_id,
            service_request_id_string,
            quoted_price_lamports
        )
    }

    pub fn submit_pastel_txid(
        ctx: Context<SubmitPastelTxid>,
        service_request_id: String,
        pastel_txid: String,
    ) -> ProgramResult {
        submit_pastel_txid_from_bridge_node_helper(ctx, service_request_id, pastel_txid)
    }

    pub fn set_oracle_contract(ctx: Context<SetOracleContract>, oracle_contract_pubkey: Pubkey) -> Result<()> {
        SetOracleContract::set_oracle_contract(ctx, oracle_contract_pubkey)
    }

    pub fn withdraw_funds(
        ctx: Context<WithdrawFunds>, 
        reward_pool_amount: u64, 
        fee_receiving_amount: u64,
        escrow_amount: u64,
        bridge_nodes_data_amount: u64,
        temp_service_requests_data_amount: u64,
        aggregated_consensus_data_amount: u64,
        service_request_txid_mapping_data_amount: u64
    ) -> Result<()> {
        WithdrawFunds::execute(
            ctx, 
            reward_pool_amount, 
            fee_receiving_amount,
            escrow_amount,
            bridge_nodes_data_amount,
            temp_service_requests_data_amount,
            aggregated_consensus_data_amount,
            service_request_txid_mapping_data_amount
        )}
        
    pub fn process_oracle_data(
        ctx: Context<AccessOracleData>,
        txid: String,
        service_request_id: String,
    ) -> Result<()> {
        let bridge_escrow_account = &mut ctx.accounts.bridge_escrow_account.to_account_info().clone();
        let system_program = &ctx.accounts.system_program.clone();
        let service_request_txid_mapping_data_account = &ctx.accounts.service_request_txid_mapping_data_account.clone();

        ctx.accounts.process(
            txid,
            &service_request_id,
            bridge_escrow_account,
            system_program,
            service_request_txid_mapping_data_account,
        )
    }
}
