pub mod big_number;
pub mod fixed_exp;
pub mod fixed_giga;

use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hash;
use anchor_lang::solana_program::program::invoke;
use anchor_lang::solana_program::sysvar::{clock::Clock, rent::Rent};
use anchor_lang::solana_program::system_instruction;
use anchor_lang::Result;

use std::convert::TryInto;

// Fixed point constants
pub const ZERO: u64 = 0;
pub const ONE: u64 = 1_000_000_000; // 1 with 9 decimal places
pub const TWO: u64 = 2_000_000_000; // 2 with 9 decimal places
pub const BITS_ONE: u64 = 0x40000000; // 1 << 30

// Bridge program specific fixed-point constants
pub const MIN_COMPLIANCE_SCORE_FOR_REWARD: u64 = 65_000_000_000; // 65.0 in fixed point
pub const MIN_RELIABILITY_SCORE_FOR_REWARD: u64 = 80_000_000_000; // 80.0 in fixed point
pub const MAX_COMPLIANCE_SCORE: u64 = 100_000_000_000; // 100.0 in fixed point
pub const RELIABILITY_RATIO_THRESHOLD: u64 = 800_000_000; // 0.8 in fixed point

// Time-based constants
const MAX_QUOTE_RESPONSE_TIME: u64 = 600; // Max time for bridge nodes to respond with a quote in seconds (10 minutes)
const QUOTE_VALIDITY_DURATION: u64 = 21_600; // The time for which a submitted price quote remains valid. e.g., 6 hours in seconds
const ESCROW_DURATION: u64 = 7_200; // Duration to hold SOL in escrow in seconds (2 hours); if the service request is not fulfilled within this time, the SOL is refunded to the user and the bridge node won't receive any payment even if they fulfill the request later.
const DURATION_IN_SECONDS_TO_WAIT_AFTER_ANNOUNCING_NEW_PENDING_SERVICE_REQUEST_BEFORE_SELECTING_BEST_QUOTE: u64 = 300; // amount of time to wait after advertising pending service requests to select the best quote
const TRANSACTION_FEE_PERCENTAGE: u8 = 2; // Percentage of the selected (best) quoted price in SOL to be retained as a fee by the bridge contract for each successfully completed service request; the rest should be paid to the bridge node. If the bridge node fails to fulfill the service request, the full escrow  is refunded to the user without any deduction for the service fee. 
const BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS: u64 = 1_000_000; // Registration fee for bridge nodes in lamports
const SERVICE_REQUEST_VALIDITY: u64 = 86_400; // Time until a service request expires if not responded to. e.g., 24 hours in seconds
const BRIDGE_NODE_INACTIVITY_THRESHOLD: u64 = 86_400; // e.g., 24 hours in seconds
const SERVICE_REQUESTS_FOR_PERMANENT_BAN: u32 = 250; // 
const SERVICE_REQUESTS_FOR_TEMPORARY_BAN: u32 = 50; // Considered for temporary ban after 50 service requests
const TEMPORARY_BAN_SERVICE_FAILURES_THRESHOLD: u32 = 5; // Number of non-consensus report submissions for temporary ban
const TEMPORARY_BAN_DURATION: u64 = 24 * 60 * 60; // Duration of temporary ban in seconds (e.g., 1 day)
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
    #[msg("Maximum account size exceeded")]
    MaxSizeExceeded,
    #[msg("Account initialization failed")]
    AccountInitializationFailed,
    #[msg("Invalid account size")]
    InvalidAccountSize,
    #[msg("Account is not rent exempt")]
    NotRentExempt,
    #[msg("PDA derivation failed")]
    PdaDerivationFailed,
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
    pub compliance_score: u64, // The compliance score of the bridge node, which is a combined measure of the bridge node's overall track record of performing services quickly, accurately, and reliably.
    pub reliability_score: u64, // The reliability score of the bridge node, which is the percentage of all attempted service requests that were completed successfully by the bridge node.
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

impl Default for ServiceRequest {
    fn default() -> Self {
        ServiceRequest {
            service_request_id: String::new(),
            service_type: PastelTicketType::Sense, // Default value
            first_6_characters_of_sha3_256_hash_of_corresponding_file: String::new(),
            ipfs_cid: String::new(),
            file_size_bytes: 0,
            user_sol_address: Pubkey::default(),
            status: RequestStatus::Pending,
            payment_in_escrow: false,
            request_expiry: 0,
            sol_received_from_user_timestamp: None,
            selected_bridge_node_pastelid: None,
            best_quoted_price_in_lamports: None,
            service_request_creation_timestamp: 0,
            bridge_node_selection_timestamp: None,
            bridge_node_submission_of_txid_timestamp: None,
            submission_of_txid_to_oracle_timestamp: None,
            service_request_completion_timestamp: None,
            payment_received_timestamp: None,
            payment_release_timestamp: None,
            escrow_amount_lamports: None,
            service_fee_retained_by_bridge_contract_lamports: None,
            pastel_txid: None,
        }
    }
}

// This holds the information for an individual price quote from a given bridge node for a particular service request.
// Price quote structure for bridge node responses
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub struct ServicePriceQuote {
    pub service_request_id: String,
    pub bridge_node_pastel_id: String,
    pub quoted_price_lamports: u64,
    pub quote_timestamp: u64,
    pub price_quote_status: ServicePriceQuoteStatus,
}

// Mapping between service requests and Pastel transaction IDs
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub struct ServiceRequestTxidMapping {
    pub service_request_id: String,
    pub pastel_txid: String,
}

// Hash weight structure for consensus data
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub struct HashWeight {
    pub hash: String,
    pub weight: i32,
}

// Consensus data structure from oracle contract
#[derive(Debug, Clone, PartialEq, Eq, Hash, AnchorSerialize, AnchorDeserialize)]
pub struct AggregatedConsensusData {
    pub txid: String,
    pub status_weights: [i32; TXID_STATUS_VARIANT_COUNT],
    pub hash_weights: Vec<HashWeight>,
    pub first_6_characters_of_sha3_256_hash_of_corresponding_file: String,
    pub last_updated: u64,
}

// Account to receive registration fees
#[account]
#[derive(Default, PartialEq, Eq)]
pub struct RegFeeReceivingAccount {}

// Main contract state account
#[account]
#[derive(Default, PartialEq, Eq)]
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


// Reward pool account for bridge nodes
#[account]
#[derive(Default, PartialEq, Eq)]
pub struct BridgeRewardPoolAccount {}

// Escrow account for holding SOL during service requests
#[account]
#[derive(Default, PartialEq, Eq)]
pub struct BridgeEscrowAccount {}

// Account holding bridge node data
#[account]
#[derive(Default, PartialEq, Eq)]
pub struct BridgeNodesDataAccount {
    pub bridge_nodes: Vec<BridgeNode>,
}

// Account for txid mappings
#[account]
#[derive(Default, PartialEq, Eq)]
pub struct ServiceRequestTxidMappingDataAccount {
    pub mappings: Vec<ServiceRequestTxidMapping>,
}

// Account for temporary service request data
#[account]
#[derive(Default, PartialEq, Eq)]
pub struct TempServiceRequestsDataAccount {
    pub service_requests: Vec<ServiceRequest>,
}

// Account for aggregated consensus data
#[account]
#[derive(Default, PartialEq, Eq)]
pub struct AggregatedConsensusDataAccount {
    pub consensus_data: Vec<AggregatedConsensusData>,
}

// Implementation for accounts that only hold SOL
impl BridgeRewardPoolAccount {
    pub fn new() -> Self {
        Self {}
    }
}

impl BridgeEscrowAccount {
    pub fn new() -> Self {
        Self {}
    }
}

impl RegFeeReceivingAccount {
    pub fn new() -> Self {
        Self {}
    }
}

// Implementations for data-holding accounts
impl BridgeNodesDataAccount {
    pub fn new() -> Self {
        Self {
            bridge_nodes: Vec::new(),
        }
    }
}

impl ServiceRequestTxidMappingDataAccount {
    pub fn new() -> Self {
        Self {
            mappings: Vec::new(),
        }
    }
}

impl TempServiceRequestsDataAccount {
    pub fn new() -> Self {
        Self {
            service_requests: Vec::new(),
        }
    }
}

impl AggregatedConsensusDataAccount {
    pub fn new() -> Self {
        Self {
            consensus_data: Vec::new(),
        }
    }
}

// Implementation for main contract state
impl BridgeContractState {
    pub fn new() -> Self {
        Self {
            is_initialized: false,
            is_paused: false,
            admin_pubkey: Pubkey::default(),
            oracle_contract_pubkey: Pubkey::default(),
            bridge_reward_pool_account_pubkey: Pubkey::default(),
            bridge_escrow_account_pubkey: Pubkey::default(),
            bridge_nodes_data_account_pubkey: Pubkey::default(),
            temp_service_requests_data_account_pubkey: Pubkey::default(),
            aggregated_consensus_data_account_pubkey: Pubkey::default(),
            service_request_txid_mapping_account_pubkey: Pubkey::default(),
            reg_fee_receiving_account_pubkey: Pubkey::default(),
        }
    }
}

// First stage initialization accounts
#[derive(Accounts)]
#[instruction(admin_pubkey: Pubkey)]
pub struct InitializeBase<'info> {
    #[account(
        init,
        payer = user,
        space = 8 +  // Discriminator
                1 +  // is_initialized
                1 +  // is_paused
                32 + // admin_pubkey
                32 + // oracle_contract_pubkey
                32 + // bridge_reward_pool_account_pubkey
                32 + // bridge_escrow_account_pubkey
                32 + // bridge_nodes_data_account_pubkey
                32 + // temp_service_requests_data_account_pubkey
                32 + // aggregated_consensus_data_account_pubkey
                32 + // service_request_txid_mapping_account_pubkey
                32   // reg_fee_receiving_account_pubkey
    )]
    pub bridge_contract_state: Account<'info, BridgeContractState>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeCorePDAs<'info> {
    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,
    
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        init,
        seeds = [b"bridge_reward_pool_account"],
        bump,
        payer = user,
        space = 8
    )]
    pub bridge_reward_pool_account: Account<'info, BridgeRewardPoolAccount>,
    
    #[account(
        init,
        seeds = [b"bridge_escrow_account"],
        bump,
        payer = user,
        space = 8
    )]
    pub bridge_escrow_account: Account<'info, BridgeEscrowAccount>,

    #[account(
        init,
        seeds = [b"reg_fee_receiving_account"],
        bump,
        payer = user,
        space = 8
    )]
    pub reg_fee_receiving_account: Account<'info, RegFeeReceivingAccount>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitBridgeNodesData<'info> {
    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,
    
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        init,
        seeds = [b"bridge_nodes_data"],
        bump,
        payer = user,
        space = 8 + 4 + (10 * 280),  // Adjusted size
        rent_exempt = enforce
    )]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitTempRequestsData<'info> {
    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,
    
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        init,
        seeds = [b"temp_service_requests_data"],
        bump,
        payer = user,
        space = 8 + 4 + (10 * 512),  // Adjusted size
        rent_exempt = enforce
    )]
    pub temp_service_requests_data_account: Account<'info, TempServiceRequestsDataAccount>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitTxidMappingData<'info> {
    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,
    
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        init,
        seeds = [b"service_request_txid_map"],
        bump,
        payer = user,
        space = 8 + 4 + (10 * 96),  // Adjusted size
        rent_exempt = enforce
    )]
    pub service_request_txid_mapping_data_account: Account<'info, ServiceRequestTxidMappingDataAccount>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitConsensusData<'info> {
    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,
    
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        init,
        seeds = [b"aggregated_consensus_data"],
        bump,
        payer = user,
        space = 8 + 4 + (10 * 1024),  // Adjusted size
        rent_exempt = enforce
    )]
    pub aggregated_consensus_data_account: Account<'info, AggregatedConsensusDataAccount>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ReallocateBridgeState<'info> {
    #[account(mut, has_one = admin_pubkey @ BridgeError::UnauthorizedWithdrawalAccount)]
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
// Convert floating point to fixed point
pub const REALLOCATION_THRESHOLD_GIGA: u64 = 900_000_000; // 0.9 in fixed point notation
const ADDITIONAL_SPACE: usize = 10_240;
const MAX_SIZE: usize = 100 * 1024; // 100KB

// Function to reallocate account data
fn reallocate_account_data<'a>(
    account: &mut AccountInfo<'a>,
    current_usage: usize,
    payer: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
) -> Result<()> {
    let current_size = account.data_len();
    
    // Convert usage ratio to fixed point calculation
    let usage_ratio = (current_usage as u64)
        .checked_mul(ONE)
        .and_then(|n| n.checked_div(current_size as u64))
        .ok_or(error!(BridgeError::TimestampConversionError))?;

    if usage_ratio > REALLOCATION_THRESHOLD_GIGA {
        let new_size = std::cmp::min(current_size + ADDITIONAL_SPACE, MAX_SIZE);

        // Prevent reallocating beyond MAX_SIZE
        if new_size > MAX_SIZE {
            msg!(
                "Cannot reallocate account beyond MAX_SIZE of {} bytes.",
                MAX_SIZE
            );
            return err!(BridgeError::MaxSizeExceeded);
        }

        // Attempt to reallocate
        account.realloc(new_size, false)?;
        msg!(
            "Account reallocated from {} bytes to {} bytes.",
            current_size,
            new_size
        );

        // Calculate new rent minimum
        let rent = Rent::get()?;
        let new_rent_minimum = rent.minimum_balance(new_size);
        let current_lamports = account.lamports();
        let lamports_needed = new_rent_minimum.saturating_sub(current_lamports);

        if lamports_needed > 0 {
            invoke(
                &system_instruction::transfer(
                    payer.key,
                    account.key,
                    lamports_needed,
                ),
                &[
                    payer.clone(),
                    account.clone(),
                    system_program.clone(),
                ],
            )?;
            msg!(
                "Transferred {} lamports from payer to account to meet rent-exemption.",
                lamports_needed
            );
        }

        // Verify rent-exemption
        let updated_lamports = account.lamports();
        let is_rent_exempt = rent.is_exempt(updated_lamports, new_size);
        if !is_rent_exempt {
            msg!(
                "Account is not rent-exempt after reallocation. Required: {}, Current: {}",
                new_rent_minimum,
                updated_lamports
            );
            return err!(BridgeError::InsufficientFunds);
        }

        msg!(
            "Account is now rent-exempt with a size of {} bytes.",
            new_size
        );
    }

    Ok(())
}

impl<'info> ReallocateBridgeState<'info> {
    pub fn execute(ctx: Context<ReallocateBridgeState>) -> Result<()> {
        // Create AccountInfo references that live for the entire function
        let payer_info = ctx.accounts.admin_pubkey.to_account_info();
        let system_program_info = ctx.accounts.system_program.to_account_info();
        
        // Store all account infos that we'll need to modify
        let mut temp_requests_info = ctx.accounts.temp_service_requests_data_account.to_account_info();
        let mut bridge_nodes_info = ctx.accounts.bridge_nodes_data_account.to_account_info();
        let mut txid_mapping_info = ctx.accounts.service_request_txid_mapping_data_account.to_account_info();
        let mut consensus_data_info = ctx.accounts.aggregated_consensus_data_account.to_account_info();

        // Calculate usages
        let temp_service_requests_usage = ctx.accounts.temp_service_requests_data_account.service_requests.len() * 
            std::mem::size_of::<ServiceRequest>();
        let bridge_nodes_usage = ctx.accounts.bridge_nodes_data_account.bridge_nodes.len() * 
            std::mem::size_of::<BridgeNode>();
        let txid_mapping_usage = ctx.accounts.service_request_txid_mapping_data_account.mappings.len() * 
            std::mem::size_of::<ServiceRequestTxidMapping>();
        let consensus_data_usage = ctx.accounts.aggregated_consensus_data_account.consensus_data.len() * 
            std::mem::size_of::<AggregatedConsensusData>();

        // Perform reallocations
        reallocate_account_data(
            &mut temp_requests_info,
            temp_service_requests_usage,
            &payer_info,
            &system_program_info,
        )?;

        reallocate_account_data(
            &mut bridge_nodes_info,
            bridge_nodes_usage,
            &payer_info,
            &system_program_info,
        )?;

        reallocate_account_data(
            &mut txid_mapping_info,
            txid_mapping_usage,
            &payer_info,
            &system_program_info,
        )?;

        reallocate_account_data(
            &mut consensus_data_info,
            consensus_data_usage,
            &payer_info,
            &system_program_info,
        )?;

        msg!("All accounts reallocated and rent-exempt status ensured.");
        Ok(())
    }
}

#[account]
#[derive(Default, PartialEq, Eq)]
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
        return err!(BridgeError::BridgeNodeAlreadyRegistered);
    }

    // Retrieve mutable references to the lamport balance
    let fee_receiving_account_info = ctx.accounts.reg_fee_receiving_account.to_account_info();
    let mut fee_receiving_account_lamports = fee_receiving_account_info.try_borrow_mut_lamports()?;

    let reward_pool_account_info = ctx.accounts.bridge_reward_pool_account.to_account_info();
    let mut reward_pool_account_lamports = reward_pool_account_info.try_borrow_mut_lamports()?;

    // Check if the reg_fee_receiving_account received the registration fee
    if **fee_receiving_account_lamports < BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS {
        return err!(BridgeError::RegistrationFeeNotPaid);
    }

    msg!("Registration fee verified. Attempting to register new bridge node {}", ctx.accounts.user.key());

    // Deduct the registration fee from the reg_fee_receiving_account and add it to the reward pool account
    **fee_receiving_account_lamports = fee_receiving_account_lamports
        .checked_sub(BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS)
        .ok_or(error!(BridgeError::InsufficientFunds))?;

    **reward_pool_account_lamports = reward_pool_account_lamports
        .checked_add(BRIDGE_NODE_REGISTRATION_FEE_IN_LAMPORTS)
        .ok_or(error!(BridgeError::InsufficientFunds))?;

    let last_active_timestamp = Clock::get()?.unix_timestamp.try_into().map_err(|_| error!(BridgeError::TimestampConversionError))?;
    
    // Create and add the new bridge node with fixed point scores
    let new_bridge_node = BridgeNode {
        pastel_id: pastel_id.clone(),
        reward_address: *ctx.accounts.user.key,
        bridge_node_psl_address,
        registration_entrance_fee_transaction_signature: String::new(), // Replace with actual data if available
        compliance_score: ONE, // Initial compliance score (1.0 in fixed point)
        reliability_score: ONE, // Initial reliability score (1.0 in fixed point)
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
    pastel_ticket_type_string: &str,
    first_6_chars_of_hash: &str,
    user_sol_address: &Pubkey,
) -> String {
    let concatenated_str = format!(
        "{}{}{}",
        pastel_ticket_type_string,
        first_6_chars_of_hash,
        user_sol_address.to_string(),
    );

    // Convert the concatenated string to bytes
    let preimage_bytes = concatenated_str.as_bytes();
    // Compute hash
    let service_request_id_hash = hash(preimage_bytes);
    
    // Convert the first 12 bytes of Hash to a hex string (within 32 bytes limit for seed)
    let service_request_id_truncated = &service_request_id_hash.to_bytes()[..12];
    let hex_string: String = service_request_id_truncated.iter().map(|byte| format!("{:02x}", byte)).collect();

    // Optional logging
    msg!("Generated truncated service_request_id (hex): {}", hex_string);

    hex_string
}

// Operational accounts for the bridge contract for handling submissions of service requests from end users and price quotes and service request updates from bridge nodes:
#[account]
#[derive(Default, PartialEq, Eq)]
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
        seeds = [
            b"srq",
            &generate_service_request_id(
                &pastel_ticket_type_string,
                &first_6_chars_of_hash,
                &user.key()
            ).as_bytes()[..12]
        ],
        bump,
        space = 8 + // Discriminator
                std::mem::size_of::<ServiceRequestSubmissionAccount>()
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
    if request.first_6_characters_of_sha3_256_hash_of_corresponding_file.len() != 6 {
        return err!(BridgeError::InvalidFileHash);  
    }

    // Check if IPFS CID is present
    if request.ipfs_cid.trim().is_empty() {
        msg!("Error: IPFS CID is empty");
        return err!(BridgeError::InvalidIpfsCid);
    }

    // Check file size
    if request.file_size_bytes == 0 {
        msg!("Error: File size cannot be zero");
        return err!(BridgeError::InvalidFileSize);
    }

    // Check if the user Solana address is valid
    if request.user_sol_address == Pubkey::default() {
        msg!("Error: User Solana address is missing or invalid");
        return err!(BridgeError::InvalidUserSolAddress);
    }

    if !matches!(request.service_type, PastelTicketType::Sense | PastelTicketType::Cascade | PastelTicketType::Nft | PastelTicketType::InferenceApi) {
        msg!("Error: Invalid or unsupported service type");
        return err!(BridgeError::InvalidServiceType);
    }

    if request.status != RequestStatus::Pending {
        msg!("Error: Initial service request status must be Pending");
        return err!(BridgeError::InvalidRequestStatus);
    }

    if request.payment_in_escrow {
        msg!("Error: Payment in escrow should initially be false");
        return err!(BridgeError::InvalidPaymentInEscrow);
    }

    if request.selected_bridge_node_pastelid.is_some() || request.best_quoted_price_in_lamports.is_some() {
        msg!("Error: Initial values for certain fields must be None");
        return err!(BridgeError::InvalidInitialFieldValues);
    }

    if request.escrow_amount_lamports.is_some() || request.service_fee_retained_by_bridge_contract_lamports.is_some() {
        msg!("Error: Initial escrow amount and fees must be None or zero");
        return err!(BridgeError::InvalidEscrowOrFeeAmounts);
    }
    
    Ok(())
}

pub fn submit_service_request_helper(
    ctx: Context<SubmitServiceRequest>,
    pastel_ticket_type_string: String,
    first_6_chars_of_hash: String,
    ipfs_cid: String,
    file_size_bytes: u64,
) -> Result<()> {
    let current_timestamp = Clock::get()?.unix_timestamp as u64;

    // Convert the pastel_ticket_type_string to PastelTicketType enum
    let service_type = match pastel_ticket_type_string.as_str() {
        "Sense" => PastelTicketType::Sense,
        "Cascade" => PastelTicketType::Cascade,
        "Nft" => PastelTicketType::Nft,
        "InferenceApi" => PastelTicketType::InferenceApi,
        _ => return err!(BridgeError::InvalidServiceType),
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
        return err!(BridgeError::DuplicateServiceRequestId);
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
        request_expiry: current_timestamp + ESCROW_DURATION,
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

    validate_service_request(&service_request_account.service_request)?;        

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

#[account]
pub struct ServicePriceQuoteSubmissionAccount {
    pub price_quote: ServicePriceQuote,
    pub price_quote_status: ServicePriceQuoteStatus,
}

#[derive(Accounts)]
#[instruction(bridge_node_pastel_id: String, service_request_id: String, quoted_price_lamports: u64)]
pub struct SubmitPriceQuote<'info> {
    #[account(
        init,
        payer = user,
        seeds = [b"px_quote", service_request_id.as_bytes()],
        bump,
        space = 8 + std::mem::size_of::<ServicePriceQuoteSubmissionAccount>()
    )]
    pub price_quote_submission_account: Account<'info, ServicePriceQuoteSubmissionAccount>,

    #[account(mut)]
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    #[account(mut)]
    pub temp_service_requests_data_account: Account<'info, TempServiceRequestsDataAccount>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    #[account(
        mut,
        seeds = [b"bpx", service_request_id.as_bytes()],
        bump
    )]
    pub best_price_quote_account: Account<'info, BestPriceQuoteReceivedForServiceRequest>,
}

pub fn submit_price_quote_helper(
    ctx: Context<SubmitPriceQuote>,
    bridge_node_pastel_id: String,
    service_request_id: String,
    quoted_price_lamports: u64,
) -> Result<()> {
    let quote_timestamp = Clock::get()?.unix_timestamp as u64;

    let service_request = ctx.accounts.temp_service_requests_data_account.service_requests.iter()
        .find(|request| request.service_request_id == service_request_id)
        .ok_or_else(|| error!(BridgeError::ServiceRequestNotFound))?;

    if quote_timestamp < service_request.service_request_creation_timestamp + 
        DURATION_IN_SECONDS_TO_WAIT_AFTER_ANNOUNCING_NEW_PENDING_SERVICE_REQUEST_BEFORE_SELECTING_BEST_QUOTE {
        msg!("Quote submission is too early. Please wait until the waiting period is over.");
        return err!(BridgeError::QuoteResponseTimeExceeded);
    }

    validate_price_quote_submission(
        &ctx.accounts.bridge_nodes_data_account,
        &ctx.accounts.temp_service_requests_data_account,
        &ctx.accounts.price_quote_submission_account,
        service_request_id.clone(),
        bridge_node_pastel_id.clone(),
        quoted_price_lamports,
        quote_timestamp,
    )?;

    let submitting_bridge_node = ctx.accounts.bridge_nodes_data_account.bridge_nodes.iter_mut()
        .find(|node| node.reward_address == ctx.accounts.user.key())
        .ok_or(BridgeError::UnauthorizedBridgeNode)?;

    submitting_bridge_node.total_price_quotes_submitted += 1;

    let price_quote_account = &mut ctx.accounts.price_quote_submission_account;
    price_quote_account.price_quote = ServicePriceQuote {
        service_request_id,
        bridge_node_pastel_id: submitting_bridge_node.pastel_id.clone(),
        quoted_price_lamports,
        quote_timestamp,
        price_quote_status: ServicePriceQuoteStatus::Submitted,
    };

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
    let service_request = temp_service_requests_data_account
        .service_requests
        .iter()
        .find(|request| request.service_request_id == service_request_id)
        .ok_or(BridgeError::ServiceRequestNotFound)?;

    if service_request.status != RequestStatus::Pending {
        msg!("Error: Service request is not in a pending state");
        return err!(BridgeError::ServiceRequestNotPending);
    }

    if quote_timestamp > service_request.service_request_creation_timestamp + MAX_QUOTE_RESPONSE_TIME {
        msg!("Error: Price quote submitted beyond the maximum response time");
        return err!(BridgeError::QuoteResponseTimeExceeded);
    }

    let current_timestamp = Clock::get()?.unix_timestamp as u64;
    if quote_timestamp > service_request.service_request_creation_timestamp + 
       MAX_DURATION_IN_SECONDS_FROM_LAST_REPORT_SUBMISSION_BEFORE_SELECTING_WINNING_QUOTE {
        msg!("Error: Price quote submitted too late");
        return err!(BridgeError::QuoteResponseTimeExceeded);
    }

    let registered_bridge_node = bridge_nodes_data_account
        .bridge_nodes
        .iter()
        .find(|node| &node.pastel_id == &bridge_node_pastel_id);

    if let Some(bridge_node) = registered_bridge_node {
        if bridge_node.calculate_is_banned(current_timestamp) {
            msg!("Error: Bridge node is currently banned");
            return err!(BridgeError::BridgeNodeBanned);
        }

        // Check compliance and reliability scores using fixed point comparison
        if bridge_node.compliance_score < MIN_COMPLIANCE_SCORE_FOR_REWARD ||
           bridge_node.reliability_score < MIN_RELIABILITY_SCORE_FOR_REWARD {
            msg!("Error: Bridge node does not meet the minimum score requirements for rewards");
            return err!(BridgeError::BridgeNodeScoreTooLow);
        }

        if current_timestamp > bridge_node.last_active_timestamp + BRIDGE_NODE_INACTIVITY_THRESHOLD {
            msg!("Error: Bridge node is inactive");
            return err!(BridgeError::BridgeNodeInactive);
        }
    } else {
        msg!("Error: Bridge node is not registered");
        return err!(BridgeError::UnregisteredBridgeNode);
    }

    if quoted_price_lamports == 0 {
        msg!("Error: Quoted price cannot be zero");
        return err!(BridgeError::InvalidQuotedPrice);
    }

    if price_quote_submission_account.price_quote_status != ServicePriceQuoteStatus::Submitted {
        msg!("Error: Price quote status is not set to 'Submitted'");
        return err!(BridgeError::InvalidQuoteStatus);
    }

    if current_timestamp > price_quote_submission_account.price_quote.quote_timestamp + QUOTE_VALIDITY_DURATION {
        msg!("Error: Price quote has expired");
        return err!(BridgeError::QuoteExpired);
    }

    Ok(())
}
pub fn choose_best_price_quote(
    best_quote_account: &BestPriceQuoteReceivedForServiceRequest,
    temp_service_requests_data_account: &mut TempServiceRequestsDataAccount,
    current_timestamp: u64
) -> Result<()> {
    let service_request = temp_service_requests_data_account.service_requests
        .iter_mut()
        .find(|request| request.service_request_id == best_quote_account.service_request_id)
        .ok_or(error!(BridgeError::ServiceRequestNotFound))?;

    if current_timestamp > best_quote_account.best_quote_timestamp + QUOTE_VALIDITY_DURATION {
        return err!(BridgeError::QuoteExpired);
    }

    service_request.selected_bridge_node_pastelid = Some(best_quote_account.best_bridge_node_pastel_id.clone());
    service_request.best_quoted_price_in_lamports = Some(best_quote_account.best_quoted_price_in_lamports);
    service_request.bridge_node_selection_timestamp = Some(current_timestamp);
    service_request.status = RequestStatus::BridgeNodeSelected;

    Ok(())
}

fn update_txid_mapping(
    txid_mapping_data_account: &mut Account<ServiceRequestTxidMappingDataAccount>, 
    service_request_id: String, 
    pastel_txid: String
) -> Result<()> {  // Changed from ProgramResult to Result<()>
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

    #[account(
        mut,
        constraint = bridge_contract_state.is_initialized @ BridgeError::ContractNotInitialized
    )]
    pub bridge_contract_state: Account<'info, BridgeContractState>,

    #[account(mut)]
    pub bridge_nodes_data_account: Account<'info, BridgeNodesDataAccount>,

    #[account(
        mut,
        seeds = [b"service_request_txid_map"],
        bump
    )]
    pub service_request_txid_mapping_data_account: Account<'info, ServiceRequestTxidMappingDataAccount>,

    pub system_program: Program<'info, System>,
}

pub fn submit_pastel_txid_from_bridge_node_helper(
    ctx: Context<SubmitPastelTxid>, 
    service_request_id: String, 
    pastel_txid: String
) -> Result<()> {  // Changed from ProgramResult to Result<()>
    let service_request = &mut ctx.accounts.service_request_submission_account.service_request;

    // Validate that the service_request_id matches
    if service_request.service_request_id != service_request_id {
        return err!(BridgeError::InvalidServiceRequestId);
    }

    // Validate the format and length of the Pastel TxID
    if pastel_txid.is_empty() || pastel_txid.len() > MAX_TXID_LENGTH {
        return err!(BridgeError::InvalidPastelTxid);
    }

    // Find the bridge node that matches the selected_pastel_id
    let selected_bridge_node = ctx.accounts.bridge_nodes_data_account.bridge_nodes
        .iter()
        .find(|node| node.pastel_id == *service_request.selected_bridge_node_pastelid.as_ref().unwrap())
        .ok_or_else(|| error!(BridgeError::UnregisteredBridgeNode))?;

    // Check if the transaction is being submitted by the selected bridge node
    if selected_bridge_node.reward_address != ctx.accounts.system_program.key() {
        return err!(BridgeError::UnauthorizedBridgeNode);
    }

    // Update the service request with the provided Pastel TxID
    service_request.pastel_txid = Some(pastel_txid.clone());
    service_request.bridge_node_submission_of_txid_timestamp = Some(Clock::get()?.unix_timestamp as u64);

    // Change the status of the service request to indicate that the TxID has been submitted
    service_request.status = RequestStatus::AwaitingCompletionConfirmation;

    // Log the TxID submission
    msg!("Pastel TxID submitted by Bridge Node {} for Service Request ID: {}", 
        selected_bridge_node.pastel_id, 
        service_request_id.to_string()
    );

    // Update the TxID mapping for the service request
    let txid_mapping_data_account = &mut ctx.accounts.service_request_txid_mapping_data_account;
    update_txid_mapping(txid_mapping_data_account, service_request_id, pastel_txid)?;

    Ok(())
}

#[derive(Accounts)]
pub struct AccessOracleData<'info> {
    #[account(
        seeds = [b"aggregated_consensus_data"],
        bump
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

    /// CHECK: This account is provided by the user and is manually checked in the program logic
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

        // Update error propagation using ? operator and err! macro
        let service_request = self.temp_service_requests_data_account
            .service_requests
            .iter()
            .find(|request| &request.service_request_id == service_request_id)
            .ok_or_else(|| error!(BridgeError::ServiceRequestNotFound))?;

        if service_request.user_sol_address != *self.user_account.key {
            return err!(BridgeError::InvalidUserSolAddress);
        }

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
        let escrow_amount = escrow_amount_lamports.ok_or_else(|| error!(BridgeError::EscrowNotFunded))?;
    
        if is_success {
            // Calculate the service fee
            let service_fee = escrow_amount
                .checked_mul(TRANSACTION_FEE_PERCENTAGE.into())
                .and_then(|v| v.checked_div(100))
                .ok_or_else(|| error!(BridgeError::TimestampConversionError))?;
    
            let amount_to_bridge_node = escrow_amount
                .checked_sub(service_fee)
                .ok_or_else(|| error!(BridgeError::TimestampConversionError))?;
    
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
    
            msg!(
                "Service Request ID: {} completed successfully! Sent {} SOL to Bridge Node at address: {} and transferred the service fee of {} SOL to the Bridge Reward Pool",
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
    
            msg!(
                "Service Request ID: {} failed. Refunded {} SOL to user at address: {}",
                service_request_id,
                escrow_amount as f64 / LAMPORTS_PER_SOL as f64,
                user_sol_address,
            );
        }
    
        Ok(())
    }

}


// Refactored to be standalone functions
    pub fn send_sol<'info>(
        from_account: &AccountInfo<'info>,
        to_pubkey: Pubkey,
        amount: u64,
        system_program: &Program<'info, System>,
    ) -> Result<()> {
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
        ).map_err(Into::into)
    }

    pub fn refund_escrow_to_user(
        escrow_account: &mut AccountInfo<'_>, 
        user_account: &mut AccountInfo<'_>, 
        refund_amount: u64
    ) -> Result<()> {
        // Check if escrow account has enough balance
        if **escrow_account.lamports.borrow() < refund_amount {
            return err!(BridgeError::InsufficientEscrowFunds);
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
    let hours_inactive = time_diff / 3600;
    
    let success_scaling = BridgeNode::to_fixed(
        (1.0 + bridge_node.current_streak as f32 * 0.1).min(2.0)
    );
    
    // Calculate and apply time weight to scale scores based on activity
    let time_weight = BridgeNode::to_fixed(
        1.0 / (1.0 + hours_inactive as f32 / 480.0)
    );
    
    let score_increment = ((BridgeNode::to_fixed(20.0) * success_scaling) / BridgeNode::SCORE_SCALE)
        .checked_mul(time_weight)
        .and_then(|n| n.checked_div(BridgeNode::SCORE_SCALE))
        .unwrap_or(0);

    let score_decrement = BridgeNode::to_fixed(
        20.0 * (1.0 + bridge_node.failed_service_requests_count as f32 * 0.5).min(3.0)
    );
    
    // Apply decay rate based on inactivity
    let decay_rate = BridgeNode::to_fixed(0.99);
    let decay_factor = BridgeNode::to_fixed(
        0.99f32.powf(hours_inactive as f32 / 24.0)
    );
    
    let streak_bonus = if success {
        BridgeNode::to_fixed((bridge_node.current_streak as f32 / 10.0).min(3.0).max(0.0))
    } else {
        0
    };
    
    if success {
        bridge_node.successful_service_requests_count += 1;
        bridge_node.current_streak += 1;
        
        // Apply time-weighted score increment
        let weighted_increment = (score_increment * time_weight) / BridgeNode::SCORE_SCALE;
        bridge_node.compliance_score = bridge_node.compliance_score
            .saturating_add(weighted_increment)
            .saturating_add(streak_bonus);
    } else {
        bridge_node.current_streak = 0;
        bridge_node.compliance_score = bridge_node.compliance_score
            .saturating_sub(score_decrement);
    }
    
    // Apply decay rate
    bridge_node.compliance_score = ((bridge_node.compliance_score * decay_rate) / BridgeNode::SCORE_SCALE)
        .checked_mul(decay_factor)
        .and_then(|n| n.checked_div(BridgeNode::SCORE_SCALE))
        .unwrap_or(bridge_node.compliance_score);
    
    let reliability_factor = if bridge_node.total_service_requests_attempted > 0 {
        (bridge_node.successful_service_requests_count as u64 * BridgeNode::SCORE_SCALE) / 
         bridge_node.total_service_requests_attempted as u64
    } else {
        0
    };
    
    // Apply time weight to final compliance score
    bridge_node.compliance_score = ((bridge_node.compliance_score * reliability_factor) / BridgeNode::SCORE_SCALE)
        .checked_mul(time_weight)
        .and_then(|n| n.checked_div(BridgeNode::SCORE_SCALE))
        .unwrap_or(bridge_node.compliance_score)
        .min(BridgeNode::to_fixed(100.0));
    
    bridge_node.reliability_score = reliability_factor;
    
    // Update reward eligibility
    bridge_node.is_eligible_for_rewards = bridge_node.compliance_score >= MIN_COMPLIANCE_SCORE_FOR_REWARD 
        && bridge_node.reliability_score >= MIN_RELIABILITY_SCORE_FOR_REWARD;
    
    log_score_updates(bridge_node);
}

pub fn logistic_scale(score: u64, max_value: u64, steepness: u64, midpoint: u64) -> u64 {
    let exp_term = ((score as i64 - midpoint as i64) * steepness as i64) / 
                   BridgeNode::SCORE_SCALE as i64;
    let denom = BridgeNode::SCORE_SCALE + 
                BridgeNode::to_fixed((-exp_term as f32).exp());
    (max_value * BridgeNode::SCORE_SCALE) / denom
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

// Update the update_statuses function to use fixed-point arithmetic
pub fn update_statuses(bridge_node: &mut BridgeNode, current_timestamp: u64) {
    // Update the recently active status based on the last active timestamp and the inactivity threshold
    bridge_node.is_recently_active = current_timestamp - bridge_node.last_active_timestamp < BRIDGE_NODE_INACTIVITY_THRESHOLD;

    // Update the reliability status based on the ratio of successful to attempted service requests
    bridge_node.is_reliable = if bridge_node.total_service_requests_attempted > 0 {
        let success_ratio = (bridge_node.successful_service_requests_count as u64)
            .checked_mul(BridgeNode::SCORE_SCALE)
            .and_then(|n| n.checked_div(bridge_node.total_service_requests_attempted as u64))
            .unwrap_or(0);
        
        // Compare using fixed-point arithmetic
        success_ratio >= MIN_RELIABILITY_SCORE_FOR_REWARD
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
    const SCORE_SCALE: u64 = 1_000_000_000; // 9 decimal places
    
    /// Converts a floating point value to fixed point representation
    fn to_fixed(float_val: f32) -> u64 {
        (float_val * Self::SCORE_SCALE as f32) as u64
    }
    
    /// Converts a fixed point value back to floating point
    /// 
    /// This function is provided for completeness of the fixed-point API
    /// and debugging/display purposes
    #[allow(dead_code)]
    fn from_fixed(fixed_val: u64) -> f32 {
        fixed_val as f32 / Self::SCORE_SCALE as f32
    }

    /// Calculate if the bridge node is currently banned based on timestamp
    pub fn calculate_is_banned(&self, current_timestamp: u64) -> bool {
        if self.ban_expiry == u64::MAX {
            // Permanent ban
            true
        } else if self.ban_expiry > current_timestamp {
            // Temporary ban still active
            true
        } else {
            // Not banned or ban expired
            false
        }
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

#[derive(Accounts)]
#[instruction(service_request_id: String)]
pub struct InitializeBestPriceQuote<'info> {
    #[account(
        init,
        payer = user,
        seeds = [b"bpx", &service_request_id.as_bytes()[..12]],
        bump,
        space = 12 + std::mem::size_of::<BestPriceQuoteReceivedForServiceRequest>()
    )]
    pub best_price_quote_account: Account<'info, BestPriceQuoteReceivedForServiceRequest>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub system_program: Program<'info, System>,
}

impl<'info> InitializeBestPriceQuote<'info> {
    pub fn initialize(ctx: Context<InitializeBestPriceQuote>, service_request_id: String) -> Result<()> {
        let best_price_quote_account = &mut ctx.accounts.best_price_quote_account;

        // Store the full service_request_id
        best_price_quote_account.service_request_id = service_request_id;
        best_price_quote_account.best_bridge_node_pastel_id = "".to_string();
        best_price_quote_account.best_quoted_price_in_lamports = 0;
        best_price_quote_account.best_quote_timestamp = 0;
        best_price_quote_account.best_quote_selection_status = BestQuoteSelectionStatus::NoQuotesReceivedYet;

        Ok(())
    }
}

declare_id!("Ew8ohkPJ3JnWoZ3MWvkn86wYMRJkS385Bsis9TwQJo79");
#[program]
pub mod solana_pastel_bridge_program {
    use super::*;

       pub fn initialize_base(ctx: Context<InitializeBase>, admin_pubkey: Pubkey) -> Result<()> {
        msg!("Initializing Bridge Contract Base State");
        require!(!ctx.accounts.bridge_contract_state.is_initialized, BridgeError::ContractStateAlreadyInitialized);

        let state = &mut ctx.accounts.bridge_contract_state;
        state.is_initialized = true;
        state.is_paused = false;
        state.admin_pubkey = admin_pubkey;
        state.oracle_contract_pubkey = Pubkey::default();

        msg!("Bridge Contract Base State Initialized with Admin Pubkey: {:?}", admin_pubkey);
        Ok(())
    }

    pub fn initialize_core_pdas(ctx: Context<InitializeCorePDAs>) -> Result<()> {
        msg!("Initializing Core PDAs");
        
        // Ensure bridge contract state is initialized
        require!(
            ctx.accounts.bridge_contract_state.is_initialized,
            BridgeError::ContractNotInitialized
        );

        let state = &mut ctx.accounts.bridge_contract_state;
        
        // Initialize the reward pool account
        msg!("Initializing reward pool account...");
        state.bridge_reward_pool_account_pubkey = ctx.accounts.bridge_reward_pool_account.key();
        
        // Initialize the escrow account
        msg!("Initializing escrow account...");
        state.bridge_escrow_account_pubkey = ctx.accounts.bridge_escrow_account.key();
        
        // Initialize the registration fee account
        msg!("Initializing registration fee account...");
        state.reg_fee_receiving_account_pubkey = ctx.accounts.reg_fee_receiving_account.key();

        // Log successful initialization
        msg!(
            "Core PDAs initialized with reward pool: {}, escrow: {}, reg fee: {}",
            ctx.accounts.bridge_reward_pool_account.key(),
            ctx.accounts.bridge_escrow_account.key(),
            ctx.accounts.reg_fee_receiving_account.key()
        );

        Ok(())
    }

    pub fn initialize_bridge_nodes_data(ctx: Context<InitBridgeNodesData>) -> Result<()> {
        msg!("Initializing bridge nodes data account");
        ctx.accounts.bridge_nodes_data_account.bridge_nodes = Vec::with_capacity(10);
        
        let state = &mut ctx.accounts.bridge_contract_state;
        state.bridge_nodes_data_account_pubkey = ctx.accounts.bridge_nodes_data_account.key();
        
        Ok(())
    }

    pub fn initialize_temp_requests_data(ctx: Context<InitTempRequestsData>) -> Result<()> {
        msg!("Initializing temp service requests account");
        ctx.accounts.temp_service_requests_data_account.service_requests = Vec::with_capacity(10);
        
        let state = &mut ctx.accounts.bridge_contract_state;
        state.temp_service_requests_data_account_pubkey = ctx.accounts.temp_service_requests_data_account.key();
        
        Ok(())
    }

    pub fn initialize_txid_mapping_data(ctx: Context<InitTxidMappingData>) -> Result<()> {
        msg!("Initializing txid mapping account");
        ctx.accounts.service_request_txid_mapping_data_account.mappings = Vec::with_capacity(10);
        
        let state = &mut ctx.accounts.bridge_contract_state;
        state.service_request_txid_mapping_account_pubkey = ctx.accounts.service_request_txid_mapping_data_account.key();
        
        Ok(())
    }

    pub fn initialize_consensus_data(ctx: Context<InitConsensusData>) -> Result<()> {
        msg!("Initializing consensus data account");
        ctx.accounts.aggregated_consensus_data_account.consensus_data = Vec::with_capacity(10);
        
        let state = &mut ctx.accounts.bridge_contract_state;
        state.aggregated_consensus_data_account_pubkey = ctx.accounts.aggregated_consensus_data_account.key();
        
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
        register_new_bridge_node_helper(ctx, pastel_id, bridge_node_psl_address)
    }

    pub fn submit_service_request(
        ctx: Context<SubmitServiceRequest>,
        pastel_ticket_type_string: String,
        first_6_chars_of_hash: String,
        ipfs_cid: String,
        file_size_bytes: u64,
    ) -> Result<()> {  // Change from ProgramResult
        submit_service_request_helper(ctx, pastel_ticket_type_string, first_6_chars_of_hash, ipfs_cid, file_size_bytes)
    }

    pub fn initialize_best_price_quote(
        ctx: Context<InitializeBestPriceQuote>,
        service_request_id: String,
    ) -> Result<()> {
        InitializeBestPriceQuote::initialize(ctx, service_request_id)
    }

    pub fn submit_price_quote(
        ctx: Context<SubmitPriceQuote>,
        bridge_node_pastel_id: String,
        service_request_id_string: String,
        quoted_price_lamports: u64,
    ) -> Result<()> {
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
    ) -> Result<()> {
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