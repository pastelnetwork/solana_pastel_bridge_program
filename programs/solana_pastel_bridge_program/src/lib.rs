use anchor_lang::prelude::*;
use anchor_lang::solana_program::entrypoint::ProgramResult;
use anchor_lang::solana_program::account_info::AccountInfo;
use anchor_lang::solana_program::sysvar::clock::Clock;
use anchor_lang::solana_program::hash::{hash, Hash};

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
#[instruction(service_request_id_string: String, bridge_node_pastel_id: Pubkey)]
pub struct SubmitPriceQuote<'info> {
    #[account(
        init,
        payer = user,
        seeds = [b"price_quote", &hex_string_to_bytes(&service_request_id_string)?, bridge_node_pastel_id.as_ref()],
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
    pub bridge_node_pastel_id: Pubkey,

    // Account to store the best price quote for a service request
    #[account(
        mut,
        seeds = [b"best_price_quote", &hex_string_to_bytes(&service_request_id_string)?],
        bump
    )]
    pub best_price_quote_account: Account<'info, BestPriceQuoteReceivedForServiceRequest>,
}

impl<'info> SubmitPriceQuote<'info> {
    pub fn submit_price_quote(
        ctx: Context<SubmitPriceQuote>,
        service_request_id_string: String,
        bridge_node_pastel_id: Pubkey,
        quoted_price_lamports: u64,
        quote_timestamp: u64,
    ) -> ProgramResult {
        // Convert the hexadecimal string to bytes
        let service_request_id = hex_string_to_bytes(&service_request_id_string)?;

        // Determine the PDA for the price quote
        let (price_quote_pda, _bump_seed) = Pubkey::find_program_address(
            &[b"price_quote", &service_request_id, bridge_node_pastel_id.as_ref()],
            ctx.program_id,
        );

        // Check if the PDA account already exists
        match ctx.accounts.system_program.get_account_info(&price_quote_pda) {
            Ok(_) => {
                // If the account exists, reject the submission
                return Err(BridgeError::DuplicatePriceQuote.into());
            }
            Err(_) => {
                // If the account does not exist, continue processing
            }
        }

        // Create or update the ServicePriceQuoteSubmissionAccount
        let price_quote_account = &mut ctx.accounts.price_quote_submission_account;
        price_quote_account.price_quote = ServicePriceQuote {
            service_request_id,
            bridge_node_pastel_id,
            quoted_price_lamports,
            quote_timestamp,
            price_quote_status: ServicePriceQuoteStatus::Submitted,
        };

        // Update best price quote if the new quote is better
        let best_quote_account = &mut ctx.accounts.best_price_quote_account;
        if best_quote_account.best_quote_selection_status == BestQuoteSelectionStatus::NoQuotesReceivedYet ||
        quoted_price_lamports < best_quote_account.best_quoted_price_in_lamports {
            best_quote_account.best_bridge_node_pastel_id = bridge_node_pastel_id.to_string();
            best_quote_account.best_quoted_price_in_lamports = quoted_price_lamports;
            best_quote_account.best_quote_timestamp = quote_timestamp;
            best_quote_account.best_quote_selection_status = BestQuoteSelectionStatus::BestQuoteSelected;
        }

        Ok(())
    }
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


pub fn apply_bans(contributor: &mut Contributor, current_timestamp: u64, is_accurate: bool) {
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
    } else {
        // Create new data
        let mut new_data = AggregatedConsensusData {
            txid: txid.to_string(),
            status_weights: [0; TXID_STATUS_VARIANT_COUNT],
            hash_weights: Vec::new(),
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
        return Err(BridgeError::EnoughReportsSubmittedForTxid.into());
    }    

    // Validate the data report before any contributor-specific checks
    // msg!("Validating data report: {:?}", report);
    validate_data_contributor_report(&report)?;

    // Check if the contributor is registered and not banned
    // msg!("Checking if contributor is registered and not banned");    
    let contributor = contributor_data_account.contributors
        .iter()
        .find(|c| c.reward_address == contributor_reward_address)
        .ok_or(BridgeError::ContributorNotRegistered)?;

    if contributor.calculate_is_banned(Clock::get()?.unix_timestamp as u64) {
        return Err(BridgeError::ContributorBanned.into());
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
        return Err(BridgeError::PendingPaymentAlreadyInitialized.into());
    }

    // Ensure txid is correct and other fields are properly set
    if pending_payment.txid != txid {
        return Err(BridgeError::InvalidTxid.into());
    }

    // Store the pending payment in the account
    pending_payment_account.pending_payment = pending_payment;

    msg!("Pending payment account initialized: TXID: {}, Expected Amount: {}, Status: {:?}", 
        pending_payment_account.pending_payment.txid, 
        pending_payment_account.pending_payment.expected_amount, 
        pending_payment_account.pending_payment.payment_status);

    Ok(())
}



#[account]
pub struct BridgeNodeDataAccount {
    pub bridge_nodes: Vec<BridgeNode>,
}

#[account]
pub struct TxidSubmissionCountsAccount {
    pub submission_counts: Vec<TxidSubmissionCount>,
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
        return Err(BridgeError::PaymentNotFound.into());
    }

    // Verify the payment amount matches the expected amount
    if pending_payment_account.pending_payment.expected_amount != amount {
        return Err(BridgeError::InvalidPaymentAmount.into());
    }

    // Mark the payment as received
    pending_payment_account.pending_payment.payment_status = PaymentStatus::Received;

    Ok(())
}


#[derive(Accounts)]
pub struct WithdrawFunds<'info> {
    #[account(
        mut,
        constraint = oracle_contract_state.admin_pubkey == *admin_account.key @ BridgeError::UnauthorizedWithdrawalAccount,
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
            return Err(BridgeError::UnauthorizedWithdrawalAccount.into()); // Check if the admin_account is a signer
        } 
        let admin_account = &mut ctx.accounts.admin_account;
        let reward_pool_account = &mut ctx.accounts.reward_pool_account;
        let fee_receiving_contract_account = &mut ctx.accounts.fee_receiving_contract_account;

        // Transfer SOL from the reward pool account to the admin account
        if **reward_pool_account.to_account_info().lamports.borrow() < reward_pool_amount {
            return Err(BridgeError::InsufficientFunds.into());
        }
        **reward_pool_account.to_account_info().lamports.borrow_mut() -= reward_pool_amount;
        **admin_account.lamports.borrow_mut() += reward_pool_amount;

        // Transfer SOL from the fee receiving contract account to the admin account
        if **fee_receiving_contract_account.to_account_info().lamports.borrow() < fee_receiving_amount {
            return Err(BridgeError::InsufficientFunds.into());
        }
        **fee_receiving_contract_account.to_account_info().lamports.borrow_mut() -= fee_receiving_amount;
        **admin_account.lamports.borrow_mut() += fee_receiving_amount;

        msg!("Withdrawal successful: {} lamports transferred from reward pool and {} lamports from fee receiving contract to admin account", reward_pool_amount, fee_receiving_amount);
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

    pub fn reallocate_oracle_state(ctx: Context<ReallocateOracleState>) -> Result<()> {
        ReallocateOracleState::execute(ctx)
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

    pub fn add_txid_for_monitoring(ctx: Context<AddTxidForMonitoring>, data: AddTxidForMonitoringData) -> Result<()> {
        add_txid_for_monitoring_helper(ctx, data)
    }

    pub fn add_pending_payment(ctx: Context<HandlePendingPayment>, txid: String, expected_amount_str: String, payment_status_str: String) -> Result<()> {
        let expected_amount = expected_amount_str.parse::<u64>()
            .map_err(|_| BridgeError::PendingPaymentInvalidAmount)?;
    
        // Convert the payment status from string to enum
        let payment_status = match payment_status_str.as_str() {
            "Pending" => PaymentStatus::Pending,
            "Received" => PaymentStatus::Received,
            _ => return Err(BridgeError::InvalidPaymentStatus.into()),
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
            _ => return Err(ProgramError::from(BridgeError::InvalidTxidStatus))
        };
    
        let pastel_ticket_type = match pastel_ticket_type_str.as_str() {
            "Sense" => PastelTicketType::Sense,
            "Cascade" => PastelTicketType::Cascade,
            "Nft" => PastelTicketType::Nft,
            "InferenceApi" => PastelTicketType::InferenceApi,
            _ => return Err(ProgramError::from(BridgeError::InvalidPastelTicketType))
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