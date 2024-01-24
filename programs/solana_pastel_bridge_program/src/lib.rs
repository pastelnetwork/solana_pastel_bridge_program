use anchor_lang::prelude::*;
use anchor_lang::solana_program::entrypoint::ProgramResult;
use anchor_lang::solana_program::account_info::AccountInfo;
use anchor_lang::solana_program::sysvar::clock::Clock;
use anchor_lang::solana_program::hash::{hash, Hash};
use oracle_program::{self, cpi::accounts::AddTxidForMonitoring};
use oracle_program::AddTxidForMonitoringData;


const COST_IN_LAMPORTS_OF_ADDING_PASTEL_TXID_FOR_MONITORING: u64 = 100_000; // 0.0001 SOL in lamports
const MAX_QUOTE_RESPONSE_TIME: u64 = 600; // Max time for bridge nodes to respond with a quote in seconds (10 minutes)
const QUOTE_VALIDITY_DURATION: u64 = 21_600; // The time for which a submitted price quote remains valid. e.g., 6 hours in seconds
const ESCROW_DURATION: u64 = 7_200; // Duration to hold SOL in escrow in seconds (2 hours); if the service request is not fulfilled within this time, the SOL is refunded to the user and the bridge node won't receive any payment even if they fulfill the request later.
const DURATION_IN_SECONDS_TO_WAIT_BEFORE_CHECKING_TXID_STATUS_AFTER_SUBMITTING_TO_ORACLE_CONTRACT: u64 = 300; // amount of time to wait before checking the status of a txid submitted to the oracle contract
const DURATION_IN_SECONDS_TO_WAIT_AFTER_ANNOUNCING_NEW_PENDING_SERVICE_REQUEST_BEFORE_SELECTING_BEST_QUOTE: u64 = 300; // amount of time to wait after advertising pending service requests to select the best quote
const TRANSACTION_FEE_PERCENTAGE: u8 = 2; // Percentage of transaction fee
const ORACLE_REWARD_PERCENTAGE: u8 = 20; // Percentage of the transaction fee allocated to the oracle contract
const BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS: u64 = 1_000_000; // Registration fee for bridge nodes in lamports
const SERVICE_REQUEST_VALIDITY: u64 = 86_400; // Time until a service request expires if not responded to. e.g., 24 hours in seconds
const BRIDGE_NODE_INACTIVITY_THRESHOLD: u64 = 86_400; // e.g., 24 hours in seconds
const MIN_COMPLIANCE_SCORE_FOR_REWARD: f32 = 65.0; // Bridge Node must have a compliance score of at least N to be eligible for rewards
const MIN_RELIABILITY_SCORE_FOR_REWARD: f32 = 80.0; // Minimum reliability score to be eligible for rewards
const SERVICE_REQUESTS_FOR_PERMANENT_BAN: u32 = 250; // 
const SERVICE_REQUESTS_FOR_TEMPORARY_BAN: u32 = 50; // Considered for temporary ban after 50 service requests
const TEMPORARY_BAN_SERVICE_FAILURES_THRESHOLD: u32 = 5; // Number of non-consensus report submissions for temporary ban
const TEMPORARY_BAN_DURATION: u64 =  24 * 60 * 60; // Duration of temporary ban in seconds (e.g., 1 day)
const BASE_REWARD_AMOUNT_IN_LAMPORTS: u64 = 100_000; // 0.0001 SOL in lamports is the base reward amount
const MAX_DURATION_IN_SECONDS_FROM_LAST_REPORT_SUBMISSION_BEFORE_SELECTING_WINNING_QUOTE: u64 = 2 * 60; // Maximum duration in seconds from service quote request before selecting the best quote (e.g., 2 minutes)
const DATA_RETENTION_PERIOD: u64 = 24 * 60 * 60; // How long to keep data in the contract state (1 day)
const SUBMISSION_COUNT_RETENTION_PERIOD: u64 = 24 * 60 * 60; // Number of seconds to retain submission counts (i.e., 24 hours)
const TXID_STATUS_VARIANT_COUNT: usize = 4; // Manually define the number of variants in TxidStatus
const MAX_TXID_LENGTH: usize = 64; // Maximum length of a TXID
const MAX_ALLOWED_TIMESTAMP_DIFFERENCE: u64 = 600; // 10 minutes in seconds; maximum allowed time difference between the last update of the oracle's consensus data and the bridge contract's access to this data to ensure the bridge contract acts on timely and accurate data.


#[error_code]
pub enum BridgeError {
    #[msg("Bridge node is already registered")]
    BridgeNodeAlreadyRegistered,

    #[msg("Action attempted by an unregistered bridge node")]
    UnregisteredBridgeNode,

    #[msg("Service request is invalid or malformed")]
    InvalidServiceRequest,

    #[msg("Service request not fulfilled within specified time limit")]
    ServiceRequestTimeout,

    #[msg("Bridge node submitted a quote that is too high or unreasonable")]
    QuoteTooHigh,

    #[msg("Bridge node submitted an invalid or malformed price quote")]
    InvalidQuote,

    #[msg("Escrow account for service request is not adequately funded")]
    EscrowNotFunded,

    #[msg("Invalid or unrecognized escrow account address")]
    InvalidEscrowAddress,

    #[msg("Transaction fee specified is invalid or not within acceptable limits")]
    InvalidTransactionFee,

    #[msg("File size exceeds the maximum allowed limit")]
    InvalidFileSize,

    #[msg("Service request has not been funded by the user")]
    ServiceRequestNotFunded,

    #[msg("Invalid or unsupported service type in service request")]
    InvalidServiceType,

    #[msg("Submitted price quote has expired and is no longer valid")]
    QuoteExpired,

    #[msg("Service request has expired due to lack of fulfillment or response")]
    RequestExpired,

    #[msg("SOL payment amount does not match required or quoted amount")]
    InvalidPaymentAmount,

    #[msg("Invalid or inappropriate response by a bridge node to a service request")]
    InvalidBridgeNodeResponse,

    #[msg("Issues or failures in interacting with the oracle contract")]
    OracleContractError,

    #[msg("Bridge node fails to meet the minimum reliability score")]
    ReliabilityScoreViolation,

    #[msg("Bridge node is inactive based on defined inactivity threshold")]
    BridgeNodeInactive,

    #[msg("Insufficient funds in escrow to cover the transaction")]
    InsufficientEscrowFunds,

    #[msg("Transaction fee exceeds specified limits")]
    ExcessiveTransactionFee,

    #[msg("Errors related to operations of the service request queue")]
    InvalidRequestQueueOperation,

    #[msg("Bridge node has not paid the required registration fee")]
    RegistrationFeeNotPaid,

    #[msg("Invalid or unrecognized Pastel transaction ID")]
    InvalidTxidStatus,

    #[msg("Duplicate service request ID")]
    DuplicateServiceRequestId,

    #[msg("Duplicate price quote for service request sent by the same bridge node")]
    DuplicatePriceQuote,

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
}


impl From<BridgeError> for ProgramError {
    fn from(e: BridgeError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

// Function to convert a hexadecimal string to a [u8; 32] array
fn hex_string_to_bytes(hex_string: &str) -> [u8; 32] {
    let mut bytes = [0u8; 32];

    if hex_string.len() != 64 {
        // If the string length is incorrect, log an error and return zeros
        eprintln!("Error: Hex string must be 64 characters long");
        return bytes;
    }

    for (i, chunk) in hex_string.chars().collect::<Vec<char>>().chunks(2).enumerate() {
        // Combine two characters into a slice and parse
        let pair = chunk.iter().collect::<String>();
        match u8::from_str_radix(&pair, 16) {
            Ok(b) => bytes[i] = b,
            Err(_) => {
                // Log an error and return zeros if there's an invalid hex character
                eprintln!("Error: Invalid hex character in chunk: {}", pair);
                return bytes;
            }
        };
    }

    bytes
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
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
}

// These are the requests for services on Pastel Network submitted by end users; they are stored in the active service requests account.
// These requests initially come in with the type of service requested, the file hash, the IPFS CID, the file size, and the file MIME type;
// Then the bridge nodes submit price quotes for the service request, and the contract selects the best quote and selects a bridge node to fulfill the request.
// The end user then pays the quoted amount in SOL to the Bridge contract, which holds this amount in escrow until the service is completed successfully.
// The selected bridge node then performs the service and submits the Pastel transaction ID to the bridge contract when it's available; the bridge contract then
// submits the Pastel transaction ID to the oracle contract for monitoring. When the oracle contract confirms the status of the transaction as being mined and activated,
// the bridge contract confirms the service confirms that the ticket has been activated and that the file referenced in the ticket matches the file hash submitted in the
// service request. If the file hash matches, the escrowed SOL is released to the bridge node (minus the service fee paid to the Bridge contract) and the service request is marked as completed.
// If the file hash does not match, or if the oracle contract does not confirm the transaction as being mined and activated within the specified time limit, the escrowed SOL is refunded to the end user.
// If the bridge node fails to submit the Pastel transaction ID within the specified time limit, the service request is marked as failed and the escrowed SOL is refunded to the end user.
// The retained service fee (assuming a successful service request) is distributed to the the bridge contract's reward pool, with a portion sent to the oracle contract's reward pool.
// The bridge node's scores are then updated based on the outcome of the service request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub struct ServiceRequest {
    pub service_request_id: [u8; 32], // Unique identifier for the service request; SHA256 hash of (service_type_as_str + first_6_characters_of_sha3_256_hash_of_corresponding_file + user_sol_address) expressed as a 32-byte array.
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
    pub service_fee_remitted_to_oracle_contract_by_bridge_contract_lamports: Option<u64>, // Amount of SOL remitted to the oracle contract by the bridge contract as a service fee, taken from the escrowed amount when the service request is completed.
    pub pastel_txid: Option<String>, // The Pastel transaction ID for the service request once it is created.
}

// This holds the information for an individual price quote from a given bridge node for a particular service request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize, Copy)]
pub struct ServicePriceQuote {
    pub service_request_id: [u8; 32],
    pub bridge_node_pastel_id: Pubkey,
    pub quoted_price_lamports: u64,
    pub quote_timestamp: u64,
    pub price_quote_status: ServicePriceQuoteStatus,
}

// Struct to hold final consensus of the txid's status from the oracle contract
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub struct AggregatedConsensusData {
    pub txid: String,
    pub status_weights: [i32; TXID_STATUS_VARIANT_COUNT],
    pub hash_weights: Vec<HashWeight>,
    pub first_6_characters_of_sha3_256_hash_of_corresponding_file: String,
    pub last_updated: u64, // Unix timestamp indicating the last update time
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct ServiceRequestTxidMapping {
    pub service_request_id: [u8; 32],
    pub pastel_txid: String,
}

#[account]
pub struct BridgeContractState {
    pub is_initialized: bool,
    pub is_paused: bool,
    pub admin_pubkey: Pubkey,
    pub oracle_contract_pubkey: Pubkey,    
    pub oracle_fee_receiving_account_pubkey: Pubkey,
    pub bridge_reward_pool_account_pda: Pubkey,
    pub bridge_escrow_account_pda: Pubkey,
    pub bridge_nodes_data_pda: Pubkey,
    pub temp_service_requests_data_pda: Pubkey,
    pub aggregated_consensus_data_pda: Pubkey,
    pub service_request_txid_mapping_account_pda: Pubkey,
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
    pub reward_pool_account: Account<'info, BridgeRewardPoolAccount>,

    #[account(init, seeds = [b"bridge_escrow_account"], bump, payer = user, space = 1024)]
    pub bridge_escrow_account: Account<'info, BridgeEscrowAccount>,

    #[account(init, seeds = [b"bridge_nodes_data"], bump, payer = user, space = 10_240)]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    #[account(init, seeds = [b"temp_service_requests_data"], bump, payer = user, space = 10_240)]
    pub temp_service_requests_data_account: Account<'info, TempServiceRequestsDataAccount>,

    #[account(init, seeds = [b"aggregated_consensus_data"], bump, payer = user, space = 10_240)]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,

    #[account(init, seeds = [b"service_request_txid_mapping_data"], bump, payer = user, space = 10_240)]
    pub service_request_txid_mapping_data_account: Account<'info, ServiceRequestTxidMappingDataAccount>,

    // System program is needed for account creation
    pub system_program: Program<'info, System>,    
}

impl<'info> Initialize<'info> {
    pub fn initialize_bridge_state(&mut self, admin_pubkey: Pubkey) -> Result<()> {
        msg!("Setting up Bridge Contract State");

        let state = &mut self.oracle_contract_state;
        // Ensure the bridge_contract_state is not already initialized
        if state.is_initialized {
            return Err(BridgeError::AccountAlreadyInitialized.into());
        }
        state.is_initialized = true;
        state.is_paused = false;
        state.admin_pubkey = admin_pubkey;
        msg!("Admin Pubkey set to: {:?}", admin_pubkey);

        // Link to necessary PDAs
        state.bridge_reward_pool_account = self.bridge_reward_pool_account_pda.key();
        state.bridge_escrow_account = self.bridge_escrow_account_pda.key();
        state.bridge_nodes_data_account = self.bridge_nodes_data_pda.key();
        state.temp_service_requests_data_account = self.temp_service_requests_data_pda.key();
        state.aggregated_consensus_data_account = self.aggregated_consensus_data_pda.key();

        state.oracle_contract_pubkey = Pubkey::default();
        msg!("Oracle Contract Pubkey set to default");

        state.oracle_fee_receiving_account = Pubkey::default();
        msg!("Oracle Fee Receiving Account Pubkey set to default");

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

// Function to generate the service_request_id based on the service type, first 6 characters of the file hash, and the end user's Solana address from which they will be paying for the service.
pub fn generate_service_request_id(
    pastel_ticket_type_string: &String,
    first_6_chars_of_hash: &String,
    user_sol_address: &Pubkey,
) -> [u8; 32] {
    let user_sol_address_str = user_sol_address.to_string();
    let concatenated_str = format!(
        "{}{}{}",
        pastel_ticket_type_string,
        first_6_chars_of_hash,
        user_sol_address_str,
    );

    // Log the concatenated string
    msg!("Concatenated string for service_request_id: {}", concatenated_str);

    let hash_result: Hash = hash(concatenated_str.as_bytes());
    let service_request_id = hash_result.to_bytes();

    // Manually convert hash to hexadecimal string
    let hex_string: String = service_request_id.iter().map(|byte| format!("{:02x}", byte)).collect();
    msg!("Generated service_request_id (hex): {}", hex_string);

    service_request_id
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
        seeds = [b"service_request_data", generate_service_request_id(
            &pastel_ticket_type_string, // Pass references directly
            &first_6_chars_of_hash,
            &user.key()
        ).as_ref()],
        bump,
        space = 8 + std::mem::size_of::<ServiceRequest>()
    )]
    pub service_request_submission_account: Account<'info, ServiceRequestSubmissionAccount>,

    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(mut, seeds = [b"temp_service_requests_data"], bump)]
    pub temp_service_request_data_account: Account<'info, TempServiceRequestsDataAccount>,

    #[account(mut, seeds = [b"aggregated_consensus_data"], bump)]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,

    pub system_program: Program<'info, System>,

    // Instruction arguments
    pub pastel_ticket_type_string: String,
    pub first_6_chars_of_hash: String,
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

    if request.escrow_amount_lamports.is_some() || request.service_fee_retained_by_bridge_contract_lamports.is_some() || request.service_fee_remitted_to_oracle_contract_by_bridge_contract_lamports.is_some() {
        msg!("Error: Initial escrow amount and fees must be None or zero");
        return Err(BridgeError::InvalidEscrowOrFeeAmounts.into());
    }
    
    Ok(())
}

impl<'info> SubmitServiceRequest<'info> {
    pub fn submit_service_request(ctx: Context<SubmitServiceRequest>, ipfs_cid: String, file_size_bytes: u64) -> ProgramResult {
        let current_timestamp = Clock::get()?.unix_timestamp as u64;

        // Convert the pastel_ticket_type_string to PastelTicketType enum
        let service_type: PastelTicketType = match ctx.accounts.pastel_ticket_type_string.as_str() {
            "Sense" => PastelTicketType::Sense,
            "Cascade" => PastelTicketType::Cascade,
            "Nft" => PastelTicketType::Nft,
            "InferenceApi" => PastelTicketType::InferenceApi,
            _ => return Err(ProgramError::InvalidArgument),
        };

        // Generate the service_request_id
        let service_request_id = generate_service_request_id(
            &ctx.accounts.pastel_ticket_type_string,
            &ctx.accounts.first_6_chars_of_hash,
            &ctx.accounts.user.key(),
        );

        // Determine the PDA for the service request
        let (service_request_pda, _bump_seed) = Pubkey::find_program_address(
            &[b"service_request_data", &service_request_id],
            ctx.program_id,
        );

        // Check if the PDA account already exists
        match ctx.accounts.system_program.get_account_info(&service_request_pda) {
            Ok(_) => {
                // If the account exists, reject the submission
                return Err(BridgeError::DuplicateServiceRequestId.into());
            }
            Err(_) => {
                // If the account does not exist, continue processing
            }
        }        

        // Create or update the ServiceRequest struct
        let mut service_request_account = &mut ctx.accounts.service_request_submission_account;
        service_request_account.service_request = ServiceRequest {
            service_request_id,
            service_type,
            first_6_characters_of_sha3_256_hash_of_corresponding_file: ctx.accounts.first_6_chars_of_hash.clone(),
            ipfs_cid,
            file_size_bytes,
            user_sol_address: *ctx.accounts.user.key,
            status: RequestStatus::Pending,
            payment_in_escrow: false,
            request_expiry: 0, // Set appropriate expiry timestamp
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
            service_fee_remitted_to_oracle_contract_by_bridge_contract_lamports: None,
            pastel_txid: None,
        };

        // Call validate_service_request function
        validate_service_request(&service_request_account.service_request)?;        

        // Append the new service request to the temp_service_requests_data_account
        let temp_service_requests_account = &mut ctx.accounts.temp_service_requests_data_account;
        temp_service_requests_account.service_requests.push(service_request_account.service_request.clone());

        Ok(())
    }
}

#[account]
pub struct BestPriceQuoteReceivedForServiceRequest {
    pub service_request_id: [u8; 32],
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
pub struct SubmitPriceQuote<'info> {
    #[account(
        init,
        payer = user,
        seeds = [b"price_quote", &hex_string_to_bytes(&service_request_id_string)?],
        bump,
        space = 8 + std::mem::size_of::<ServicePriceQuote>()
    )]
    pub price_quote_submission_account: Account<'info, ServicePriceQuoteSubmissionAccount>,

    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    #[account(mut)]
    pub user: Signer<'info>,

    // System program for account creation
    pub system_program: Program<'info, System>,

    // Instruction arguments
    pub service_request_id_string: String,

    // Bridge Nodes Data Account
    #[account(mut)]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    // Account to store the best price quote for a service request
    #[account(
        mut,
        seeds = [b"best_price_quote", &hex_string_to_bytes(&service_request_id_string)?],
        bump
    )]
    pub best_price_quote_account: Account<'info, BestPriceQuoteReceivedForServiceRequest>,
}

pub fn validate_price_quote_submission(
    bridge_nodes_data_account: &BridgeNodesDataAccount, // Add this parameter
    temp_service_requests_account: &TempServiceRequestsDataAccount,
    price_quote_submission_account: &ServicePriceQuoteSubmissionAccount,
    service_request_id: &[u8; 32],
    bridge_node_pastel_id: &Pubkey,
    quoted_price_lamports: u64,
    quote_timestamp: u64,
) -> Result<()> {

    // Check if the service_request_id corresponds to an existing and pending service request
    let service_request = temp_service_requests_account
        .service_requests
        .iter()
        .find(|request| request.service_request_id == *service_request_id)
        .ok_or(BridgeError::ServiceRequestNotFound)?;

    // Validate service request status
    if service_request.status != RequestStatus::Pending {
        msg!("Error: Service request is not in a pending state");
        return Err(BridgeError::ServiceRequestNotPending.into());
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
        .find(|node| &node.reward_address == bridge_node_pastel_id);

    if let Some(bridge_node) = registered_bridge_node {
        // Check if the bridge node is banned
        if bridge_node.is_banned() {
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
    if registered_bridge_node.compliance_score < MIN_COMPLIANCE_SCORE_FOR_REWARD || registered_bridge_node.reliability_score < MIN_RELIABILITY_SCORE_FOR_REWARD {
        msg!("Error: Bridge node does not meet the minimum score requirements for rewards");
        return Err(BridgeError::BridgeNodeScoreTooLow.into());
    }

    // Bridge Node Activity Status
    if current_timestamp > registered_bridge_node.last_active_timestamp + BRIDGE_NODE_INACTIVITY_THRESHOLD {
        msg!("Error: Bridge node is inactive");
        return Err(BridgeError::BridgeNodeInactive.into());
    }

    // Bridge Node Ban Status
    if registered_bridge_node.is_temporarily_banned(current_timestamp) || registered_bridge_node.is_permanently_banned() {
        msg!("Error: Bridge node is banned");
        return Err(BridgeError::BridgeNodeBanned.into());
    }

    Ok(())
}


impl<'info> SubmitPriceQuote<'info> {
    pub fn submit_price_quote(
        ctx: Context<SubmitPriceQuote>,
        service_request_id_string: String,
        quoted_price_lamports: u64,
    ) -> ProgramResult {
        // Convert the hexadecimal string to bytes
        let service_request_id = hex_string_to_bytes(&service_request_id_string)?;

        // Obtain the current timestamp
        let current_timestamp = Clock::get()?.unix_timestamp as u64;

        // Find the bridge node that is submitting the quote
        let submitting_bridge_node = ctx.accounts.bridge_nodes_data_account.bridge_nodes.iter().find(|node| {
            node.reward_address == ctx.accounts.user.key()
        }).ok_or(BridgeError::UnauthorizedBridgeNode)?;

        // Validate the price quote before processing
        validate_price_quote_submission(
            &ctx.accounts.bridge_nodes_data_account, 
            &ctx.accounts.temp_service_requests_data_account, 
            &ctx.accounts.price_quote_submission_account, 
            &service_request_id, 
            &submitting_bridge_node.pastel_id, 
            quoted_price_lamports, 
            current_timestamp
        )?;

        // Update the total number of price quotes submitted by the bridge node
        submitting_bridge_node.total_price_quotes_submitted += 1;

        // Add the price quote to the price_quote_submission_account
        let price_quote_account = &mut ctx.accounts.price_quote_submission_account;
        price_quote_account.price_quote = ServicePriceQuote {
            service_request_id,
            bridge_node_pastel_id: submitting_bridge_node.pastel_id.clone(),
            quoted_price_lamports,
            quote_timestamp: current_timestamp, // Use internally generated timestamp
            price_quote_status: ServicePriceQuoteStatus::Submitted,
        };

        // Update best price quote if the new quote is better
        let best_quote_account = &mut ctx.accounts.best_price_quote_account;
        if best_quote_account.best_quote_selection_status == BestQuoteSelectionStatus::NoQuotesReceivedYet ||
        quoted_price_lamports < best_quote_account.best_quoted_price_in_lamports {
            best_quote_account.best_bridge_node_pastel_id = submitting_bridge_node.pastel_id.clone();
            best_quote_account.best_quoted_price_in_lamports = quoted_price_lamports;
            best_quote_account.best_quote_timestamp = current_timestamp;
            best_quote_account.best_quote_selection_status = BestQuoteSelectionStatus::BestQuoteSelected;
        }

        Ok(())
    }
}


pub fn choose_best_price_quote(
    best_quote_account: &BestPriceQuoteReceivedForServiceRequest,
    temp_service_requests_account: &mut TempServiceRequestsDataAccount,
    bridge_nodes_data_account: &BridgeNodesDataAccount,
    current_timestamp: u64
) -> Result<()> {
    // Find the service request corresponding to the best price quote
    let service_request = temp_service_requests_account.service_requests
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
    service_request_id: [u8; 32], 
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
        seeds = [b"service_request_txid_mapping_data"],
        bump
    )]
    pub service_request_txid_mapping_data_account: Account<'info, ServiceRequestTxidMappingDataAccount>,

    // System program
    pub system_program: Program<'info, System>,
}

impl<'info> SubmitPastelTxid<'info> {
    pub fn process(ctx: Context<SubmitPastelTxid>, service_request_id: [u8; 32], pastel_txid: String) -> ProgramResult {
        let service_request = &mut ctx.accounts.service_request_submission_account.service_request;

        // Validate that the service_request_id matches
        if service_request.service_request_id != service_request_id {
            return Err(BridgeError::InvalidServiceRequestId.into());
        }

        // Validate the format and length of the Pastel TxID
        if pastel_txid.is_empty() || pastel_txid.len() > MAX_TXID_LENGTH {
            return Err(BridgeError::InvalidTxid.into());
        }

        // Find the bridge node that matches the selected_pastel_id
        let selected_bridge_node = ctx.accounts.bridge_nodes_data_account.bridge_nodes.iter().find(|node| {
            node.pastel_id == service_request.selected_bridge_node_pastelid.as_ref().unwrap()
        }).ok_or(BridgeError::BridgeNodeNotFound)?;

        // Check if the transaction is being submitted by the selected bridge node
        if selected_bridge_node.reward_address != ctx.accounts.system_program.key() {
            return Err(BridgeError::UnauthorizedBridgeNode.into());
        }

        // Update the service request with the provided Pastel TxID
        service_request.pastel_txid = Some(pastel_txid);
        service_request.bridge_node_submission_of_txid_timestamp = Some(Clock::get()?.unix_timestamp as u64);

        // Change the status of the service request to indicate that the TxID has been submitted
        service_request.status = RequestStatus::TxidSubmitted;

        // Log the TxID submission
        msg!("Pastel TxID submitted by Bridge Node {} for Service Request ID: {}", selected_bridge_node.pastel_id, service_request_id.to_string());

        // Update the TxID mapping for the service request
        let txid_mapping_data_account = &mut ctx.accounts.service_request_txid_mapping_data_account;
        update_txid_mapping(txid_mapping_data_account, service_request_id, pastel_txid)?;

        Ok(())
    }
}

fn is_valid_pastel_txid(txid: &str) -> bool {
    // Example validation: check length and character set
    txid.len() == 64 && txid.chars().all(|c| c.is_ascii_hexdigit())
}


#[derive(Accounts)]
pub struct SubmitTxidToOracle<'info> {
    // Service request submission account
    #[account(mut)]
    pub service_request_submission_account: Account<'info, ServiceRequestSubmissionAccount>,

    // Bridge contract state
    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    // Oracle program account
    pub oracle_program: Program<'info, OracleProgram>,

    // Pending payment account to be initialized
    #[account(init, payer = payer, space = 8 + size_of::<PendingPaymentAccount>())]
    pub pending_payment_account: Account<'info, PendingPaymentAccount>,

    // Payer of the transaction
    #[account(mut)]
    pub payer: Signer<'info>,

    // System program
    pub system_program: Program<'info, System>,
}

impl<'info> SubmitTxidToOracle<'info> {
    pub fn process(&self, pastel_txid: String) -> Result<()> {
        let service_request = &self.service_request_submission_account.service_request;

        // Ensure the service request is ready for oracle monitoring
        if service_request.status != RequestStatus::TxidSubmitted {
            return Err(BridgeError::InvalidRequestStatus.into());
        }

        // Prepare data for oracle contract
        let data = AddTxidForMonitoringData { txid: pastel_txid };

        // Create CPI context for the oracle program
        let cpi_accounts = AddTxidForMonitoring {
            oracle_contract_state: self.oracle_program.to_account_info(),
            caller: self.service_request_submission_account.to_account_info(),
            pending_payment_account: self.pending_payment_account.to_account_info(),
            user: self.payer.to_account_info(),
            system_program: self.system_program.to_account_info(),
        };
        let cpi_program = self.oracle_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        // Invoke the oracle program's function
        oracle_program::cpi::add_txid_for_monitoring(cpi_ctx, data)?;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct AccessOracleData<'info> {
    #[account(
        seeds = [b"aggregated_consensus_data"],
        bump,
        program = bridge_contract_state.oracle_contract_pubkey,
    )]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,

    #[account(
        seeds = [b"service_request_txid_mapping_data"],
        bump
    )]
    pub service_request_txid_mapping_data_account: Account<'info, ServiceRequestTxidMappingDataAccount>,

    #[account(mut)]
    pub temp_service_requests_account: Account<'info, TempServiceRequestsDataAccount>,

    #[account]
    pub bridge_contract_state: Account<'info, BridgeContractState>,
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
        &self, 
        txid: String, 
        service_request_id: &[u8; 32], 
        service_request_txid_mapping_account: &Account<ServiceRequestTxidMappingDataAccount>
    ) -> Result<()> {
        // Search for the aggregated data matching the given txid
        if let Some(consensus_data) = get_aggregated_data(&self.aggregated_consensus_data_account, &txid) {
            // Validate the consensus data
            self.validate_consensus_data(consensus_data, service_request_id, service_request_txid_mapping_account)?;

            // Process the consensus data as required
            if self.is_txid_mined_and_file_hash_matches(consensus_data, service_request_txid_mapping_account, service_request_id) {
                // Release the escrowed amount to the bridge node and update the bridge node score
                self.handle_escrow_transactions(consensus_data, true)?;

                // Update the service request state to Completed
                self.update_service_request_state_to_completed(consensus_data)?;
            } else {
                // Handle failure case: refund escrowed funds to user and adjust bridge node score
                self.handle_escrow_transactions(consensus_data, false)?;
            }
        } else {
            // Handle the case where the txid is not found in the oracle data
            return Err(BridgeError::TxidNotFound.into());
        }

        Ok(())
    }

    fn validate_consensus_data(
        &self,
        consensus_data: &AggregatedConsensusData,
        service_request_id: &[u8; 32],
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
        service_request_id: &[u8; 32]
    ) -> bool {
        let (consensus_status, consensus_hash) = compute_consensus(consensus_data);

        if consensus_status != TxidStatus::MinedActivated {
            return false;
        }

        // Find the service request and check if the hashes match
        if let Some(mapping) = txid_mappings.mappings.iter().find(|m| m.service_request_id == *service_request_id) {
            mapping.pastel_txid == consensus_data.txid && mapping.first_6_characters_of_sha3_256_hash_of_corresponding_file == consensus_hash
        } else {
            false
        }
    }

    fn update_service_request_state_to_completed(&self, consensus_data: &AggregatedConsensusData) -> Result<()> {
        // Retrieve the mapping between service_request_id and pastel_txid
        let service_request_id = self.get_service_request_id_for_pastel_txid(consensus_data.txid.as_str())?;

        // Find the service request by ID and update its status
        let service_request = self.temp_service_requests_account.service_requests.iter_mut()
            .find(|request| request.service_request_id == service_request_id)
            .ok_or(BridgeError::ServiceRequestNotFound)?;

        // Update the status to Completed
        service_request.status = RequestStatus::Completed;
        service_request.service_request_completion_timestamp = Some(Clock::get()?.unix_timestamp);

        Ok(())
    }

    fn get_service_request_id_for_pastel_txid(&self, txid: &str) -> Result<[u8; 32]> {
        self.service_request_txid_mapping_data_account.mappings.iter()
            .find(|mapping| mapping.pastel_txid == txid)
            .map(|mapping| mapping.service_request_id)
            .ok_or(BridgeError::TxidMappingNotFound.into())
    }    

    fn handle_escrow_transactions(
        &self, 
        consensus_data: &AggregatedConsensusData, 
        success: bool
    ) -> Result<()> {
        let service_request_id = self.get_service_request_id_for_pastel_txid(&consensus_data.txid)?;
        let current_timestamp = Clock::get()?.unix_timestamp as u64;

        // Retrieve the service request
        let service_request = self.temp_service_requests_account.service_requests.iter_mut()
            .find(|request| request.service_request_id == service_request_id)
            .ok_or(BridgeError::ServiceRequestNotFound)?;

        // Retrieve the bridge node
        let bridge_node = self.bridge_nodes_data_account.bridge_nodes.iter_mut()
            .find(|node| node.pastel_id == service_request.selected_bridge_node_pastelid.clone().unwrap_or_default())
            .ok_or(BridgeError::BridgeNodeNotFound)?;

        if success {
            // Release escrowed funds to the bridge node, deducting service fees
            let service_fee = service_request.escrow_amount_lamports.unwrap_or_default() * TRANSACTION_FEE_PERCENTAGE as u64 / 100;
            let oracle_fee = service_fee * ORACLE_REWARD_PERCENTAGE as u64 / 100;
            let reward_amount = service_fee - oracle_fee;

            self.reward_pool_account.try_borrow_mut_lamports()?.checked_add(reward_amount)
                .ok_or(BridgeError::EscrowReleaseError)?;

            // Update bridge node's scores
            update_bridge_node_status(bridge_node, true, current_timestamp);

            // Mark service request as completed
            service_request.status = RequestStatus::Completed;
        } else {
            // Refund escrowed funds to the user
            let refund_amount = service_request.escrow_amount_lamports.unwrap_or_default();
            *self.user.try_borrow_mut_lamports()? = self.user.lamports().checked_add(refund_amount)
                .ok_or(BridgeError::EscrowRefundError)?;

            // Adjust bridge node's scores
            update_bridge_node_status(bridge_node, false, current_timestamp);

            // Mark service request as failed
            service_request.status = RequestStatus::Failed;
        }

        Ok(())
    }

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

fn post_consensus_tasks(
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
        current_timestamp - mapping.timestamp < SUBMISSION_COUNT_RETENTION_PERIOD
    });

    msg!("Done with post-consensus tasks!");
    Ok(())
}

#[account]
pub struct BridgeNodeDataAccount {
    pub bridge_nodes: Vec<BridgeNode>,
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
        return Err(BridgeError::UnregisteredOracle.into());
    }

    // Handle reward transfer after determining eligibility
    if is_reward_valid {
        // Transfer the reward from the reward pool to the contributor
        **ctx.accounts.reward_pool_account.to_account_info().lamports.borrow_mut() -= reward_amount;
        **ctx.accounts.oracle_contract_state.to_account_info().lamports.borrow_mut() += reward_amount;

        msg!("Paid out Valid Reward Request: Contributor: {}, Amount: {}", contributor_address, reward_amount);
    } else {

        msg!("Invalid Reward Request: Contributor: {}", contributor_address);
        return Err(BridgeError::NotEligibleForReward.into());
    }

    Ok(())
}

#[derive(Accounts)]
pub struct RegisterNewBridgeNode<'info> {
    /// CHECK: Manual checks are performed in the instruction to ensure the bridge_node_account is valid and safe to use.
    #[account(mut, signer)]
    pub bridge_node_account: AccountInfo<'info>,

    #[account(mut)]
    pub bridge_reward_pool_account: Account<'info, BridgeRewardPoolAccount>,

}

pub fn register_new_bridge_node_helper(ctx: Context<RegisterNewBridgeNode>, pastel_id: String, bridge_node_psl_address: String) -> Result<()> {
    let bridge_nodes_data_account = &mut ctx.accounts.bridge_nodes_data_account;
    msg!("Initiating new bridge node registration: {}", ctx.accounts.bridge_node_account.key());

    // Check if the bridge node is already registered
    if bridge_nodes_data_account.bridge_nodes.iter().any(|node| node.reward_address == *ctx.accounts.bridge_node_account.key) {
        msg!("Registration failed: Bridge Node already registered: {}", ctx.accounts.bridge_node_account.key);
        return Err(BridgeError::BridgeNodeAlreadyRegistered.into());
    }

    // Fee checking logic
    if ctx.accounts.bridge_node_account.lamports() < BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS {
        msg!("Registration failed: Insufficient registration fee: {}", ctx.accounts.bridge_node_account.key);
        return Err(BridgeError::InsufficientRegistrationFee.into());
    }

    // Transferring the fee to the reward pool account
    **ctx.accounts.bridge_reward_pool_account.to_account_info().lamports.borrow_mut() += BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS;
    **ctx.accounts.bridge_node_account.lamports.borrow_mut() -= BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS;

    msg!("Registration fee transferred to reward pool.");
    let last_active_timestamp = Clock::get()?.unix_timestamp as u64;

    // Create and add the new bridge node
    let new_bridge_node = BridgeNode {
        pastel_id,
        reward_address: *ctx.accounts.bridge_node_account.key,
        bridge_node_psl_address,
        registration_entrance_fee_transaction_signature: String::new(), // Replace with actual data if available
        compliance_score: 1.0, // Initial compliance score
        reliability_score: 1.0, // Initial reliability score
        last_active_timestamp,
        total_price_quotes_submitted: 0,
        total_service_requests_attempted: 0,
        successful_service_requests_count: 0,
        current_streak: 0,
        failed_service_requests_count: 0,
        ban_expiry: 0,
        is_eligible_for_rewards: false,
        is_recently_active: false,
        is_reliable: false,
    };

    // Append the new bridge node to the BridgeNodesDataAccount
    bridge_nodes_data_account.bridge_nodes.push(new_bridge_node);

    // Logging for debug purposes
    msg!("New Bridge Node successfully Registered! Complete information on the new Bridge Node: {:?}", new_bridge_node);
    Ok(())
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct AddTxidForMonitoringData {
    pub txid: String,
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
        return Err(BridgeError::InvalidTxid.into());
    } 
    // Simplified TXID status validation
    if !matches!(report.txid_status, TxidStatus::MinedActivated | TxidStatus::MinedPendingActivation | TxidStatus::PendingMining | TxidStatus::Invalid) {
        return Err(BridgeError::InvalidTxidStatus.into());
    }
    // Direct return in case of missing data, reducing nested if conditions
    if report.pastel_ticket_type.is_none() {
        msg!("Error: Missing Pastel Ticket Type");
        return Err(BridgeError::MissingPastelTicketType.into());
    }
    // Direct return in case of invalid hash, reducing nested if conditions
    if let Some(hash) = &report.first_6_characters_of_sha3_256_hash_of_corresponding_file {
        if hash.len() != 6 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            msg!("Error: Invalid File Hash Length or Non-hex characters");
            return Err(BridgeError::InvalidFileHashLength.into());
        }
    } else {
        return Err(BridgeError::MissingFileHash.into());
    }
    Ok(())
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
    pub reward_pool_account: Account<'info, BridgeRewardPoolAccount>,

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
        let reward_pool_account = &mut ctx.accounts.reward_pool_account;
        let fee_receiving_contract_account = &mut ctx.accounts.fee_receiving_contract_account;
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
        withdraw_funds(&mut reward_pool_account.to_account_info(), admin_account, reward_pool_amount)?;
        withdraw_funds(&mut fee_receiving_contract_account.to_account_info(), admin_account, fee_receiving_amount)?;
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
        msg!("Bridge Reward Pool Account PDA: {:?}", ctx.accounts.reward_pool_account.key());
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
        ipfs_cid: String,
        file_size_bytes: u64,
    ) -> ProgramResult {
        SubmitServiceRequest::submit_service_request(ctx, ipfs_cid, file_size_bytes)
    }

    pub fn submit_price_quote_wrapper(
        ctx: Context<SubmitPriceQuote>,
        service_request_id_string: String,
        quoted_price_lamports: u64,
    ) -> ProgramResult {
        SubmitPriceQuote::submit_price_quote(
            ctx,
            service_request_id_string,
            quoted_price_lamports
        )
    }
    
    pub fn request_reward(ctx: Context<RequestReward>, contributor_address: Pubkey) -> Result<()> {
        request_reward_helper(ctx, contributor_address)
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
        )
    }

}