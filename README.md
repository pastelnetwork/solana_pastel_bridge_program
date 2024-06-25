# Solana-Pastel Bridge Program
This program is a complex Solana smart contract designed for a bridge between the Solana blockchain and the Pastel Network. It manages service requests, price quotes, escrow payments, and bridge node operations. Here is a detailed explanation of its workings, step-by-step reasoning, and underlying design principles:

### Overview

The program facilitates decentralized service requests from end-users on the Solana blockchain to bridge nodes on the Pastel Network. It ensures fair pricing, escrowed payments, and reliable execution of services such as Sense, Cascade, NFT creation, and Inference API. It also includes mechanisms for node registration, service request submission, price quote submission, reward distribution, and fund withdrawal.

### Key Components and Design Principles

1. **Constants**: Define various time durations, fees, thresholds, and other parameters used throughout the contract for managing timeouts, fees, escrow durations, node compliance and reliability scores, etc.

2. **Error Handling**: Custom error codes are defined using `BridgeError` enum, which provides meaningful error messages for various failure scenarios.

3. **Enums**: Various enums define the possible states and types within the contract:
   - `TxidStatus`: Status of a Pastel transaction ID.
   - `PastelTicketType`: Types of service requests (Sense, Cascade, NFT, Inference API).
   - `ServicePriceQuoteStatus`: Status of a price quote.
   - `BestQuoteSelectionStatus`: Status of the best price quote selection process.
   - `PaymentStatus`: Status of Solana payments for service requests.
   - `EmergencyAction`: Actions that can be taken in case of an emergency.
   - `RequestStatus`: States a service request can be in during its lifecycle.

4. **Structs**:
   - `BridgeNode`: Information about a bridge node.
   - `ServiceRequest`: Details of a service request.
   - `ServicePriceQuote`: Information about a price quote for a service request.
   - `AggregatedConsensusData`: Consensus data from the oracle contract.
   - `ServiceRequestTxidMapping`: Mapping between service requests and Pastel transaction IDs.

5. **Accounts**: PDAs (Program Derived Addresses) hold various states and data required for the operation of the bridge contract:
   - `BridgeContractState`: Main state of the bridge contract.
   - `BridgeRewardPoolAccount`: Holds rewards for bridge nodes.
   - `BridgeEscrowAccount`: Holds escrowed SOL while service requests are processed.
   - `BridgeNodesDataAccount`: List of bridge nodes and their details.
   - `ServiceRequestTxidMappingDataAccount`: Maps service requests to Pastel transaction IDs.
   - `TempServiceRequestsDataAccount`: Temporarily holds service requests while they are processed.
   - `AggregatedConsensusDataAccount`: Holds consensus data about service requests.

6. **Initialization**:
   - `Initialize`: Sets up the initial state of the bridge contract, including creating PDAs and setting the admin public key.
   - `ReallocateBridgeState`: Reallocates account data if usage exceeds a certain threshold to prevent data overflow.

7. **Node Registration**:
   - `RegisterNewBridgeNode`: Registers a new bridge node by paying a registration fee and storing the node's details.

8. **Service Request Handling**:
   - `SubmitServiceRequest`: Submits a new service request, generating a unique ID and storing the request details.
   - `SubmitPriceQuote`: Submits a price quote from a bridge node for a service request and updates the best quote if applicable.
   - `SubmitPastelTxid`: Submits the Pastel transaction ID from the selected bridge node once the service is completed.
   - `AccessOracleData`: Processes the consensus data from the oracle to finalize the service request and release payments.

9. **Validation**:
   - `validate_service_request`: Ensures the service request is correctly formatted and valid.
   - `validate_price_quote_submission`: Checks the validity of a price quote submission.

10. **Utility Functions**:
    - `generate_service_request_id`: Creates a unique ID for each service request.
    - `update_txid_mapping`: Updates the mapping between service requests and Pastel transaction IDs.
    - `send_sol`: Transfers SOL between accounts.
    - `refund_escrow_to_user`: Refunds escrowed SOL to the user if the service fails.
    - `apply_bans`: Applies temporary or permanent bans to bridge nodes based on their performance.
    - `update_scores`: Updates compliance and reliability scores for bridge nodes.

11. **Emergency Actions**:
    - `SetOracleContract`: Sets the oracle contract public key.
    - `WithdrawFunds`: Allows the admin to withdraw funds from the bridge contract.

### How It Works

1. **Initialization**: The contract is initialized with the `Initialize` instruction, setting up necessary PDAs and the admin public key.

2. **Node Registration**: Bridge nodes register by paying a fee, which is added to the reward pool.

3. **Service Request Submission**: Users submit service requests, which are validated and stored temporarily.

4. **Price Quote Submission**: Bridge nodes submit price quotes, which are compared to select the best quote.

5. **Service Fulfillment**: The selected bridge node fulfills the service and submits the Pastel transaction ID.

6. **Consensus and Payment**: The oracle provides consensus data on the transaction, based on which the payment is either released to the bridge node or refunded to the user.

7. **Emergency Actions and Fund Withdrawal**: The admin can update the oracle contract or withdraw funds from the contract in case of an emergency.

---

## Setup Instructions

### Install Solana Testnet on Ubuntu

#### Install Rustup
```bash
curl https://sh.rustup.rs -sSf | sh
rustup default nightly  
rustup update nightly   
rustc --version 
```

#### Install Solana
```bash
sudo apt update && sudo apt upgrade -y && sudo apt autoremove -y  
sudo apt install libssl-dev libudev-dev pkg-config zlib1g-dev llvm clang make -y         
sh -c "$(curl -sSfL https://release.solana.com/v1.17.13/install)"      
export PATH="/home/ubuntu/.local/share/solana/install/active_release/bin:$PATH" 
source ~/.zshrc   # If you use Zsh
solana --version  
```

### Setup Anchor

```bash
sudo apt-get update && sudo apt-get upgrade && sudo apt-get install -y pkg-config build-essential libudev-dev
cargo install --git https://github.com/coral-xyz/anchor avm --locked --force
avm install latest
avm use latest
anchor --version
```

### Get Code and Test

```bash
git clone https://github.com/pastelnetwork/solana_pastel_bridge_program.git                                                                                                                                                         INT ✘  base   at 12:33:21 PM 
cd solana_pastel_bridge_program
anchor test
```

---

## Step-by-Step Narrative of How the System Works:

#### 1. Initialization

**Deployment**: 
- The smart contract, written in Rust using the Anchor framework, is compiled and deployed to the Solana blockchain. Deployment involves uploading the compiled program to the Solana cluster, where it becomes an executable entity that can process transactions and interact with accounts.

**Initialization**: 
- The `initialize` function is called by the contract's admin. This function is critical for setting up the initial state of the contract. Here's a detailed breakdown of what happens during initialization:

  1. **Creating PDAs (Program Derived Addresses)**:
     - **BridgeContractState**: This PDA holds the main state of the bridge contract, including initialization status, admin public key, and references to other PDAs.
     - **BridgeRewardPoolAccount**: A PDA dedicated to holding rewards for bridge nodes. The reward pool accumulates fees from successful service requests.
     - **BridgeEscrowAccount**: This PDA temporarily holds SOL in escrow while service requests are being processed, ensuring secure payment handling.
     - **BridgeNodesDataAccount**: Stores information about all registered bridge nodes. This includes their compliance and reliability scores, last active timestamps, and other relevant data.
     - **TempServiceRequestsDataAccount**: Temporarily holds service requests while they are in progress. This ensures that the contract can manage and track active requests efficiently.
     - **AggregatedConsensusDataAccount**: Holds consensus data from the oracle regarding the status of service requests. This data helps the contract validate service completion.
     - **ServiceRequestTxidMappingDataAccount**: Maps service requests to their corresponding Pastel transaction IDs, enabling the contract to track and verify the completion of each request.

  2. **Setting the Admin Public Key**:
     - The `admin_pubkey` parameter provided during initialization is stored in the `BridgeContractState` PDA. This public key is used to authenticate administrative actions, such as withdrawing funds or updating contract parameters.

  3. **Initializing Data Structures**:
     - The `BridgeContractState` is initialized to indicate that the contract is set up and ready to operate.
     - The other PDAs (reward pool, escrow, bridge nodes data, temporary service requests, consensus data, and txid mappings) are initialized with empty data structures. For example, `bridge_nodes` is initialized as an empty vector, ready to store bridge node information.

  This initialization process ensures that the contract has a well-defined structure and is prepared to handle service requests, manage nodes, and process payments securely.

#### 2. Registration of Bridge Nodes

**Registration Request**: 
- A new bridge node submits a `register_new_bridge_node` request. This request includes the node's `Pastel ID` and `Pastel address`, along with the necessary registration fee paid in SOL.

**Validation**: 
- The contract performs several checks to validate the registration request:
  1. **Duplicate Check**: The contract scans the existing list of registered bridge nodes in the `BridgeNodesDataAccount` to ensure the Pastel ID is not already registered.
  2. **Fee Verification**: The contract verifies that the user has provided sufficient lamports (the smallest unit of SOL) to cover the registration fee. This ensures that only serious participants register as bridge nodes.

**Storing Node Details**: 
- Upon successful validation, the contract proceeds to register the new bridge node:
  1. **Fee Transfer**: The registration fee is transferred from the user's account to the `BridgeRewardPoolAccount`. This operation increases the reward pool, which will be used to pay bridge nodes for successful service completions.
  2. **Creating Bridge Node Entry**: The contract creates a new `BridgeNode` struct containing the node's details, such as:
     - `pastel_id`: The unique identifier of the node in the Pastel network.
     - `reward_address`: The Solana address to which rewards will be sent.
     - `bridge_node_psl_address`: The Pastel address used by the node.
     - `registration_entrance_fee_transaction_signature`: Initially set to an empty string or placeholder, this can later be updated with the actual transaction signature if needed.
     - `compliance_score` and `reliability_score`: Initial scores set to 1.0, indicating a fresh start.
     - `last_active_timestamp`: The current timestamp, marking the node's registration time.
     - Other fields related to the node's performance and status, initialized to their default values (e.g., counts set to 0, booleans set to `false`).

  3. **Appending to BridgeNodesDataAccount**: The new bridge node entry is appended to the `bridge_nodes` vector in the `BridgeNodesDataAccount`. This step effectively registers the node in the contract's data structure, enabling it to participate in the network by submitting price quotes and fulfilling service requests.

### 3. Submitting Service Requests

**Service Request Creation**:
- **User Submission**: A user initiates the process by calling the `submit_service_request` function. This function requires several parameters:
  - **Service Type**: The type of service the user is requesting. This could be one of several predefined types such as Sense, Cascade, NFT, or Inference API. Each type represents a different kind of service offered by the Pastel network.
  - **File Hash**: The first 6 characters of the SHA3-256 hash of the file. This partial hash is used to identify and verify the file associated with the service request.
  - **IPFS CID**: The Content Identifier for the file on the InterPlanetary File System (IPFS). The CID ensures that the file can be retrieved and verified.
  - **File Size**: The size of the file in bytes. This helps in validating the request and ensuring that it falls within acceptable limits.

**Generating Request ID**:
- **Concatenation and Hashing**: The contract generates a unique ID for the service request by concatenating the service type, the first 6 characters of the file hash, and the user's Solana address. This concatenated string serves as a unique identifier for the request.
- **Hashing**: The concatenated string is then hashed using SHA-256 to produce a unique `service_request_id`. This ID ensures that each service request can be uniquely identified and tracked.

**Validation and Storage**:
- **Format Validation**: The contract performs several checks to ensure the validity of the submitted service request:
  - **First 6 Characters of File Hash**: The contract verifies that the first 6 characters of the file hash are valid and are in hexadecimal format.
  - **IPFS CID**: It checks that the IPFS CID is not empty and appears to be well-formed.
  - **File Size**: The contract ensures that the file size is greater than zero.
  - **User Solana Address**: It confirms that the user's Solana address is valid.
  - **Service Type**: It checks that the service type is one of the supported types.
  - **Initial Request Status**: The status of the service request must be set to `Pending` initially.
  - **Escrow Payment**: Initially, the payment for the service should not be in escrow, ensuring that users only escrow funds after a bridge node has been selected.
  - **Field Defaults**: Certain fields like selected bridge node Pastel ID, quoted price, and escrow amounts should initially be `None` or zero, indicating that these values will be populated later in the process.

- **Duplicate Check**: The contract checks if a service request with the same ID already exists in the `TempServiceRequestsDataAccount`. This prevents duplicate submissions.
  
- **Storing the Request**: 
  - **Temporary Storage**: Once the service request passes all validation checks, it is stored in the `TempServiceRequestsDataAccount`. This account temporarily holds service requests while they are being processed. 
  - **Request Details**: The stored request includes all relevant details, such as the service type, file hash, IPFS CID, file size, user Solana address, status, and timestamps. It also includes any other fields necessary for tracking the request's progress through the system.

**Example Scenario**:
1. **User Initiates Request**: Alice wants to use the Sense service to analyze a file. She calls the `submit_service_request` function with the required parameters.
2. **Generating ID**: The contract concatenates the service type (Sense), the first 6 characters of the file hash, and Alice's Solana address. This string is hashed to produce a unique `service_request_id`.
3. **Validation**: The contract checks that the hash is valid, the IPFS CID is not empty, and the file size is appropriate. It also verifies that Alice's Solana address is valid and that there is no existing request with the same ID.
4. **Storing the Request**: The contract stores the validated request in the `TempServiceRequestsDataAccount` with a status of `Pending`. Alice's request is now ready for bridge nodes to submit their price quotes.

### 4. Submitting Price Quotes

**Quote Submission**:
- **Monitoring**: Bridge nodes continuously monitor the blockchain for new service requests. When a new request is detected, nodes evaluate whether they can fulfill the service and at what price.
- **Function Call**: To submit a price quote, a bridge node calls the `submit_price_quote` function. This function requires the following parameters:
  - **Pastel ID**: The unique identifier of the bridge node within the Pastel network. This ID is used to track the node's activity and performance.
  - **Service Request ID**: The unique ID of the service request for which the quote is being submitted. This ID ensures that the quote is associated with the correct service request.
  - **Quoted Price**: The price quoted by the bridge node for fulfilling the service request, specified in lamports (the smallest unit of SOL).

**Validation**:
- **Service Request Validity**: The contract checks that the service request associated with the submitted quote exists and is in a `Pending` state. This ensures that quotes are only accepted for active requests.
- **Response Time**: The contract verifies that the quote is submitted within the maximum allowed response time (`MAX_QUOTE_RESPONSE_TIME`). This prevents nodes from submitting quotes for outdated requests.
- **Non-Zero Price**: The quoted price must be greater than zero. This ensures that bridge nodes cannot submit invalid or maliciously low quotes.
- **Node Registration and Ban Status**: The contract checks that the bridge node submitting the quote is registered and not currently banned. This involves verifying the node's status in the `BridgeNodesDataAccount` to ensure it meets compliance and reliability standards.

**Selecting the Best Quote**:
- **Initial State**: When the first quote is submitted for a service request, the `BestPriceQuoteReceivedForServiceRequest` is updated with this initial quote. This sets the baseline for subsequent comparisons.
- **Comparing Quotes**: As additional quotes are submitted, the contract compares each new quote against the current best quote. The criteria for comparison is the quoted price in lamports. If the new quote is lower than the current best quote, the contract updates the `BestPriceQuoteReceivedForServiceRequest` to reflect the new best quote.
- **Updating Best Quote**: The best quote is stored along with the corresponding bridge node's Pastel ID and the timestamp of the quote. This ensures that the contract can track and select the most cost-effective quote for the user.
- **Timeliness**: The contract ensures that the best quote is selected within a specific timeframe after the service request was announced. This prevents delays and ensures timely selection of the best quote.

**Example Scenario**:
1. **Detecting Service Requests**: Bridge node Bob detects a new service request for an NFT creation service.
2. **Submitting Quote**: Bob evaluates the request and decides he can fulfill it for 50,000 lamports. He calls the `submit_price_quote` function, providing his Pastel ID, the service request ID, and his quoted price.
3. **Validation**: The contract verifies that the service request is pending, the quote is within the allowed response time, the quoted price is non-zero, and Bob is a registered bridge node who is not banned.
4. **First Quote Submission**: Since Bob's quote is the first one submitted, it automatically becomes the best quote.
5. **Subsequent Quotes**: Later, another bridge node, Alice, submits a quote for the same service request, quoting 45,000 lamports. The contract compares Alice's quote with Bob's and updates the best quote to Alice's, as her quote is lower.
6. **Finalizing Best Quote**: After the waiting period for quotes has elapsed, the contract selects Alice's quote as the best quote for the service request.


### 5. Selecting the Bridge Node

**Waiting Period**:
- **Purpose**: The waiting period (`DURATION_IN_SECONDS_TO_WAIT_AFTER_ANNOUNCING_NEW_PENDING_SERVICE_REQUEST_BEFORE_SELECTING_BEST_QUOTE`) ensures that all interested bridge nodes have a fair chance to submit their price quotes. This encourages competition and helps ensure that users receive the best possible price for the service.
- **Implementation**: During this waiting period, the contract accumulates all submitted quotes for the service request. It uses a timer or a timestamp comparison to track the duration since the announcement of the new pending service request.
- **Completion**: Once the waiting period elapses, the contract proceeds to evaluate the accumulated quotes.

**Selecting the Best Price Quote**:
- **Comparison Logic**: The contract compares all submitted quotes based on the quoted price. The quote with the lowest price is considered the best quote.
- **Update Mechanism**: If a new quote is lower than the currently stored best quote, the contract updates the `BestPriceQuoteReceivedForServiceRequest` structure to reflect this new best quote. This structure holds details about the quote, including the bridge node's Pastel ID, the quoted price, and the timestamp of the quote submission.
- **Final Selection**: At the end of the waiting period, the contract finalizes the best quote selection. This ensures that the most cost-effective quote is chosen for the user.

**Storing Selected Node**:
- **Service Request Update**: The contract updates the `ServiceRequest` structure with details of the selected bridge node. This includes:
  - **Pastel ID**: The Pastel ID of the selected bridge node.
  - **Quoted Price**: The best-quoted price in lamports.
  - **Selection Timestamp**: The timestamp when the bridge node was selected.
- **Status Change**: The status of the service request is updated to `BridgeNodeSelected`. This status change signifies that the contract has successfully selected a bridge node to fulfill the service request.

### 6. Service Fulfillment and TxID Submission

**Service Fulfillment**:
- **Execution**: The selected bridge node begins working on the service request. For example, if the request is for an NFT creation, the node would perform the necessary steps to create the NFT.
- **Generating TxID**: Upon completing the service, the bridge node generates a Pastel transaction ID (txid). This txid represents the transaction on the Pastel network that corresponds to the completed service.

**Submitting TxID**:
- **Function Call**: The bridge node submits the generated txid to the contract using the `submit_pastel_txid_from_bridge_node_helper` function. This function requires the following parameters:
  - **Service Request ID**: The unique ID of the service request.
  - **Pastel TxID**: The transaction ID generated by the Pastel network.

**Validation and Storage**:
- **TxID Validation**: The contract performs several checks to validate the submitted txid:
  - **Service Request Match**: It verifies that the service request ID matches an existing request in the `ServiceRequest` structure.
  - **TxID Format**: The contract checks that the submitted txid is correctly formatted and not empty.
  - **Authorized Node**: The contract confirms that the txid is being submitted by the bridge node selected for the service request. This involves checking that the submitting node's Pastel ID matches the selected node in the `ServiceRequest`.

- **Updating Structures**: Once validated, the contract updates the relevant data structures:
  - **ServiceRequest**: The `ServiceRequest` structure is updated to include the submitted txid. The status of the service request is changed to `AwaitingCompletionConfirmation`, indicating that the service has been completed and is awaiting final confirmation.
  - **ServiceRequestTxidMappingDataAccount**: The contract adds a mapping between the service request ID and the submitted txid. This mapping allows the contract to track the relationship between service requests and their corresponding transactions on the Pastel network.

**Example Scenario**:
1. **Waiting Period**: After a user submits a service request, the contract waits for the predefined period to allow bridge nodes to submit their quotes.
2. **Selecting the Best Quote**: At the end of the waiting period, the contract selects the best quote based on the lowest price and updates the service request with the selected bridge node's details.
3. **Service Execution**: The selected bridge node, let's say Bob, performs the requested service (e.g., creating an NFT) and generates a Pastel txid.
4. **Submitting TxID**: Bob submits the txid to the contract using the `submit_pastel_txid_from_bridge_node_helper` function.
5. **Validation**: The contract validates the txid, ensuring it matches the service request and was submitted by the authorized bridge node.
6. **Updating Structures**: The contract updates the `ServiceRequest` and `ServiceRequestTxidMappingDataAccount` with the submitted txid and changes the service request status to `AwaitingCompletionConfirmation`.

### 7. Accessing Oracle Data and Consensus

**Fetching Consensus Data**:
- **Function Invocation**: Periodically, the contract calls the `AccessOracleData` function to retrieve updated consensus data from an oracle. This function is crucial for obtaining the most recent and accurate status of the submitted Pastel transaction IDs (txids).
- **Oracle Data**: The oracle provides detailed information, including the status and hash weights of the txids. The status indicates the current state of the txid (e.g., PendingMining, MinedPendingActivation, MinedActivated), while the hash weights help in determining the consensus regarding the transaction's legitimacy and the file's integrity.

**Computing Consensus**:
- **Aggregated Data**: The consensus data retrieved from the oracle is aggregated to form a comprehensive view of the txid's status. This involves processing the status weights and hash weights provided by the oracle.
- **Consensus Algorithm**: The contract implements a consensus algorithm that evaluates the aggregated data to determine the most likely status of the txid. The algorithm looks for the status with the highest weight, indicating the most agreed-upon state among the validators.
- **Result**: The result of this computation is a consensus status (e.g., MinedActivated) and a consensus hash, which represents the most agreed-upon file hash associated with the txid.

**Validation**:
- **TxID Match**: The contract verifies that the txid obtained from the oracle matches the txid stored in the `ServiceRequestTxidMappingDataAccount`. This ensures that the consensus data corresponds to the correct service request.
- **Recency**: The contract checks the timestamp of the consensus data to ensure it is recent and has not become outdated. This is done by comparing the last updated timestamp of the consensus data with the current time, ensuring it falls within an acceptable range (`MAX_ALLOWED_TIMESTAMP_DIFFERENCE`).
- **TxID Status**: The contract ensures that the consensus status of the txid is `MinedActivated`. This status indicates that the transaction has been mined and confirmed on the Pastel network, making it reliable for finalizing the service request.

### 8. Completing the Service Request

**Matching Hashes**:
- **Consensus Hash Check**: The contract compares the consensus hash obtained from the oracle with the hash stored in the `ServiceRequest` structure. This comparison verifies that the file referenced by the txid matches the file associated with the original service request.
- **Integrity Verification**: If the hashes match, it confirms that the service has been completed correctly, and the file's integrity is intact. This step is critical for ensuring that the service provided meets the user's expectations and the contract's requirements.

**Payment Distribution**:
- **Service Fee Calculation**: The contract calculates the service fee as a percentage (`TRANSACTION_FEE_PERCENTAGE`) of the quoted price in lamports. This fee is deducted from the escrowed amount.
- **Reward Pool Transfer**: The calculated service fee is transferred to the `BridgeRewardPoolAccount`. This account accumulates fees that can be used for rewarding bridge nodes for their participation and performance.
- **Bridge Node Payment**: The remaining amount, after deducting the service fee, is transferred to the reward address of the selected bridge node. This payment compensates the bridge node for successfully completing the service request.

**Status Update**:
- **Service Request Completion**: The status of the `ServiceRequest` is updated to `Completed`. This status change signifies that the service has been successfully fulfilled, and the necessary payments have been processed.
- **Timestamp Update**: The contract records the completion timestamp in the `ServiceRequest` structure. This timestamp marks the exact time when the service request was finalized and the payments were made.

**Example Scenario**:
1. **Periodic Data Fetching**: The contract periodically fetches consensus data from the oracle, which includes the status and hash weights of submitted txids.
2. **Consensus Computation**: The contract computes the consensus status and hash based on the oracle data. For instance, if the txid's status is `MinedActivated` and the consensus hash matches the file hash stored in the service request, the contract proceeds to the next step.
3. **Validation**: The contract validates the txid, ensuring it matches the service request and that the consensus data is recent.
4. **Payment Distribution**: Upon successful validation, the contract calculates the service fee, transfers it to the reward pool, and sends the remaining amount to the bridge node's reward address.
5. **Status Update**: The contract updates the service request status to `Completed`, indicating that the service has been fulfilled and the payments have been made.

### 9. Handling Failures and Refunds

**TxID Mismatch or Expiry**:
- **TxID Mismatch**:
  - **Detection**: If the consensus data retrieved from the oracle indicates that the submitted txid does not match the expected txid stored in the `ServiceRequestTxidMappingDataAccount`, the contract flags this as a mismatch.
  - **Action**: Upon detecting a mismatch, the contract initiates a refund process to return the escrowed payment to the user. This ensures that users are not penalized for failed or incorrect service completions.
- **Service Request Expiry**:
  - **Timeout Check**: The contract tracks the time elapsed since the service request was created. If the service request remains incomplete beyond the defined expiry duration (`SERVICE_REQUEST_VALIDITY`), it is considered expired.
  - **Refund Process**: Similar to the mismatch scenario, the contract refunds the escrowed amount to the user. This protects users from losing funds due to delays or non-completion of services by bridge nodes.

**Updating Status**:
- **Failed Status**:
  - **Condition**: A service request is marked as failed if the txid submitted by the bridge node is invalid or if the consensus data indicates a mismatch.
  - **Status Update**: The contract updates the `ServiceRequest` structure, setting its status to `Failed`. This status change indicates that the service request was not successfully completed.
- **Expired Status**:
  - **Condition**: A service request is marked as expired if it surpasses the defined validity period without completion.
  - **Status Update**: The contract updates the `ServiceRequest` structure, setting its status to `Expired`. This status change indicates that the service request was not completed within the allowed timeframe.

### 10. Node Performance and Rewards

**Updating Scores**:
- **Outcome-Based Scoring**:
  - **Success**: When a bridge node successfully completes a service request, the contract increases the node's compliance and reliability scores. The scores are adjusted based on the timeliness and accuracy of the service provided.
  - **Failure**: Conversely, if a bridge node fails to complete a service request or submits an invalid txid, the contract decreases the node's scores. The adjustment accounts for the node's failure to meet the required standards.
- **Score Calculation**: The contract employs a scoring algorithm that considers factors such as the node's current streak of successful completions, the time taken to complete requests, and the overall ratio of successful to attempted service requests. This algorithm ensures that scores accurately reflect the node's performance.

**Applying Bans**:
- **Temporary Bans**:
  - **Criteria**: A bridge node may receive a temporary ban if it fails a certain number of service requests within a specified period (`TEMPORARY_BAN_SERVICE_FAILURES_THRESHOLD`).
  - **Duration**: The contract enforces a temporary ban for a predefined duration (`TEMPORARY_BAN_DURATION`). During this time, the node cannot participate in new service requests.
- **Permanent Bans**:
  - **Criteria**: Nodes that repeatedly fail to meet performance standards and exceed a higher threshold of failed service requests (`SERVICE_REQUESTS_FOR_PERMANENT_BAN`) are permanently banned.
  - **Implementation**: The contract updates the node's status in the `BridgeNodesDataAccount`, marking it as permanently banned. This action ensures that consistently underperforming nodes are removed from the network.

**Reward Eligibility**:
- **Compliance and Reliability Thresholds**:
  - **Eligibility Check**: The contract evaluates each bridge node's compliance and reliability scores against predefined thresholds (`MIN_COMPLIANCE_SCORE_FOR_REWARD` and `MIN_RELIABILITY_SCORE_FOR_REWARD`). Nodes meeting these thresholds are eligible for rewards.
  - **Ineligibility**: Nodes failing to meet the thresholds are marked as ineligible for rewards. This incentivizes nodes to maintain high performance and reliability.
- **Reward Distribution**:
  - **Mechanism**: Eligible nodes receive rewards from the `BridgeRewardPoolAccount`. The distribution is based on the number of successful completions and the node's overall contribution to the network.

**Example Scenario**:
1. **TxID Mismatch**:
   - A bridge node submits a txid that does not match the expected txid for a service request.
   - The contract detects the mismatch through consensus data.
   - The escrowed payment is refunded to the user, and the service request status is updated to `Failed`.
2. **Service Request Expiry**:
   - A service request remains incomplete beyond the validity period.
   - The contract marks the request as `Expired` and refunds the escrowed payment to the user.
3. **Updating Scores**:
   - A bridge node successfully completes a service request within the allowed time.
   - The contract increases the node's compliance and reliability scores.
4. **Applying Bans**:
   - A bridge node fails multiple service requests within a short period.
   - The contract imposes a temporary ban, preventing the node from participating in new requests.
5. **Reward Eligibility**:
   - A bridge node consistently performs well, meeting the compliance and reliability thresholds.
   - The node is eligible for rewards, which are distributed from the `BridgeRewardPoolAccount`.

### 11. Admin Actions

**Emergency Actions**:
- **Purpose**: Emergency actions provide the admin with the ability to swiftly respond to unforeseen issues or potential security threats. This ensures the contract can be managed effectively even in adverse situations.
- **Pausing Operations**:
  - **Functionality**: The admin can temporarily pause all contract operations using an emergency action. This is useful in situations where a security breach is detected or if there's a need to perform critical maintenance.
  - **Implementation**: The contract includes a flag in the `BridgeContractState` that indicates whether the contract is paused. When the pause action is invoked, this flag is set to true, and all major functions check this flag before executing any logic. If the contract is paused, they abort execution.
- **Resuming Operations**:
  - **Functionality**: Once the issue is resolved, the admin can resume normal operations. This action resets the pause flag, allowing the contract to process requests and quotes again.
  - **Implementation**: The admin calls a specific function to reset the pause flag in the `BridgeContractState`. This restores the contract to its normal operational state.
- **Modifying Parameters**:
  - **Functionality**: The admin can modify key operational parameters of the contract, such as timeouts, fees, or thresholds. This allows for flexibility and adaptability in response to changing conditions or requirements.
  - **Implementation**: The contract includes a function that allows the admin to update various parameters stored in the `BridgeContractState`. These parameters might include the maximum quote response time, the duration for holding SOL in escrow, and other critical settings.
- **Updating Oracle Contract Public Key**:
  - **Functionality**: The admin can update the oracle contract's public key, ensuring the contract always interacts with the correct and current oracle.
  - **Implementation**: The contract has a function that allows the admin to set a new oracle public key in the `BridgeContractState`. This is essential for maintaining accurate and trustworthy consensus data.

**Withdrawing Funds**:
- **Purpose**: The admin has the ability to withdraw funds from various accounts associated with the contract. This can be necessary for redistributing rewards, covering operational costs, or other administrative needs.
- **Reward Pool Withdrawal**:
  - **Functionality**: The admin can withdraw funds from the `BridgeRewardPoolAccount`, which holds the accumulated service fees from completed service requests.
  - **Implementation**: The contract includes validation checks to ensure that only the admin can execute the withdrawal. The admin's public key, stored in the `BridgeContractState`, is used to verify their identity.
- **Escrow and Other Accounts**:
  - **Functionality**: Besides the reward pool, the admin can also withdraw from the `BridgeEscrowAccount` and other operational accounts if necessary.
  - **Implementation**: Similar to reward pool withdrawals, the contract ensures that withdrawals from these accounts are securely executed and only authorized by the admin. It checks the available balance in the accounts to ensure that sufficient funds are present before allowing the withdrawal.

### Conclusion

**Decentralized System**:
- The contract establishes a decentralized system where users can interact with bridge nodes without relying on a central authority. This decentralization is crucial for the trustless nature of blockchain applications.

**Security**:
- The contract ensures security through multiple layers of validation, including verifying service requests, price quotes, and Pastel transaction IDs. Emergency actions provide additional security measures for the admin to handle potential threats.

**Fairness**:
- Fairness is achieved through competitive quoting, ensuring users receive the best possible price for services. The scoring and banning mechanisms ensure that only reliable bridge nodes participate, maintaining the integrity of the network.

**Transparency**:
- All actions, from service requests to price quotes and admin actions, are recorded on the blockchain, ensuring transparency. Users and bridge nodes can audit the contract's operations to verify fairness and accuracy.

**Scalability**:
- The contract's modular design, with separate PDAs for different data sets and operations, allows it to handle increasing volumes of service requests and node registrations without compromising performance.

**Flexibility**:
- The ability to modify parameters, handle emergencies, and update critical components like the oracle contract public key ensures the contract can adapt to changing conditions and requirements.
