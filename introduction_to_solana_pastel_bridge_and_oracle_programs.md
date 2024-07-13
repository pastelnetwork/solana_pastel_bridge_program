# Pastel Solana Bridge and Oracle Programs

We’ve designed a multi-component system for linking together Pastel Network and Solana in such a way that regular Solana users will be able to take advantage of services offered by Pastel (such as file storage with Cascade, near-duplicate image detection and fingerprinting with Sense, inference requests across many AI modalities with Inference Layer, etc.) all by interacting with a Solana native program, and paying for services using SOL natively. Importantly, this is achieved in a completely decentralized and trustless way, without requiring any centralized server to handle requests or to link together the Pastel and Solana blockchains. Critically, all this is done without exposing Solana’s end users to any “credit risk” (i.e. a scenario in which they paid SOL for services that are then never rendered on Pastel).

The design relies on two main Solana programs which were developed using Rust and Anchor: the **Bridge** program and the **Oracle** program. There is a lot of overlap in the design and philosophy of both of these Solana programs; they are both intended to be driven by contributions from participating nodes which anyone can set up trustlessly and without permission. These contributors do not need any special status on Solana or on Pastel. For instance, they do not have to be running a valid Pastel Supernode to participate as a contributor to either the Bridge or the Oracle.

The reason for separating these two programs is so that we don’t have to rely on any Bridge node to accurately relay the status of a service request on Pastel, since this would either require us to trust a Bridge node when it claims that it completed its given task successfully (it would have an incentive to lie about this), or to trust the consensus of other Bridge nodes (these would have an incentive to also lie to hurt their “competitor” Bridge node and reduce that competitor’s reputation score). This will make more sense later once we describe what each Solana program does.

## The Bridge Program

The basic idea is that, if someone wants to become a Bridge node, all they need to do is set up some machine with a static IP address to act as a regular Pastel full node and also install the Pastel Walletnode and create and register a PastelID, so that they can interact with the Pastel Network themselves to store data in Cascade, submit images to sense, etc. They would also need some inventory of PSL coins to pay for these services on Pastel. In addition to that, they would need some way of checking the status of transactions and program events and sending transactions on Solana, which they could do either by running a Solana node or by leveraging an API based service such as [Solscan](https://solscan.io/) or [Alchemy](https://www.alchemy.com/solana) (obviously, the people running these Bridge nodes would need to supply their own API keys for these services if they want to use them).

Then, this Bridge node contributor would interact with the Solana bridge program to register as a new Bridge node; this requires them sending a modest non-refundable “entrance fee” to the Bridge program to add themselves to the list of registered Bridge Nodes. They also need to supply two additional pieces of information:

- Their PastelID — this is their persistent User ID on the Pastel Network, and is required for interacting with Cascade, Sense, Inference Layer, etc.
- Their PSL address, from which they can send PSL and receive PSL coins.

Each bridge node's data is stored in the Bridge program as a struct with the following fields:

```rust
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
```

These fields allow the Bridge program to track all relevant attributes of each Bridge Node; in particular, it allows for banning of “bad” bridge node contributors, and for reputation scoring over time based on how consistently bridge nodes completed their assigned tasks.

So now assume that we have a set of independently-run bridge node contributors that are already registered with the Bridge program. The basic flow now is that if an end user on Solana wants to interact with the Pastel network, they first submit some information to the Bridge program (which does not require any registration by the end user or payment to the Bridge program other than ordinary transaction fees, which are de minimis) about their proposed **Service Request** on Pastel. For example, a service request might be that the end user wants to store a 10mb image file in Cascade and have it be publicly accessible to all users. The end user would specify all this information, as well as additional fields that can be used by the Bridge/Oracle programs and by the end user to verify that the exact specified file was correctly stored in Cascade— e.g, the SHA256 hash of the file the end user wants to store (and also an IPFS identifier used only to transfer the file from the end user to the bridge node).

Here are all the fields that are tracked for all Service Requests by the Bridge Program:

```rust
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
```

When a service request is first submitted to the Bridge program by an end user on Solana, they basically just want to get a bunch of prices quotes in SOL terms for how much it will cost them to complete that service request; e.g., to store that 10mb file in our example. The way this happens is that the service request is validated by the Bridge program and then announced by the Bridge program. All bridge nodes are actively monitoring anything that happens with the Bridge program on Solana. Once a bridge node contributor sees that a new service request has been submitted to the bridge program, the bridge node swings into action and independently determines how much it wants to charge the end user in SOL terms to complete that service request on behalf of the end user.

So how does the bridge node contributor do this? Basically, they take the information from the service request and first determine how much it would cost in PSL terms to complete that request on Pastel Network; this is easy to do using built-in APIs in Pastel. Once it knows this, it can then look up the current PSL-USD market price on Coinmarketcap/Coingecko, add some target profit margin % on top of this, and then convert this amount into SOL terms. It would then submit a price quote to the Bridge program, which looks like this:

```rust
    pub struct ServicePriceQuote {
        pub service_request_id: String,
        pub bridge_node_pastel_id: String,
        pub quoted_price_lamports: u64,
        pub quote_timestamp: u64,
        pub price_quote_status: ServicePriceQuoteStatus,
    }
```

where we have:

```rust
    pub enum ServicePriceQuoteStatus {
        Submitted,
        RejectedAsInvalid,
        RejectedAsTooHigh,
        Accepted
    }
```

There is a designated period of time during which the Bridge program collects and aggregates these price quotes and ranks them based on the quoted price (but filters out any banned Bridge nodes that have low reputation scores based on previous poor performance). The status of this process is maintained by the Bridge program using this struct:

```rust
    pub enum BestQuoteSelectionStatus {
        NoQuotesReceivedYet,
        NoValidQuotesReceivedYet,
        WaitingToSelectBestQuote,
        BestQuoteSelected,
    }
```

The end user can then review the best (lowest) quoted price in SOL and determine if they want to move forward with the service request. This is a critical part of this system— the end user on Solana doesn’t have to worry about acquiring PSL on an exchange or otherwise, or setting up a PSL wallet, or creating a PastelID— they can simply stick to Solana and pay for everything with SOL. The Bridge node contributor is effectively acquiring any needed PSL on behalf of the end user in a transparent way so that it can facilitate the desired service request for the benefit of the end user.

If the end user thinks the best quoted price is too high, they can just walk away at this point with nothing lost. If they do want to move forward with the service request at the quoted price, then they send the quoted amount of SOL to the Bridge program to be held in escrow by the Bridge program pending successful completion of the service request by the selected Bridge node contributor on Pastel Network.

Once the select Bridge node contributor observes that this has taken place in the Bridge contract, they now know that they can move forward with the service request and that, as long as they can successfully complete the request on Pastel Network, they are sure to get paid for their trouble; that is, they won’t end up in a situation where they paid PSL to store a file for the end user in Cascade but then never received the agreed-upon SOL fee in return.

That’s because once the fee is in escrow, it will be automatically remitted to the bridge node contributor by the Bridge program once the Oracle program can establish that the status of the service request is successful— that is, that there is a pastel TXID representing the completed request on Pastel’s blockchain, and that the corresponding blockchain ticket for that TXID includes the same file hash as was specified by the user in their original request (this same procedure also applies to Sense requests, since we can verify that the SHA256 hash of the submitted image from the selected Bridge node matches the image provided by the end user on Solana). There is a bit of extra complexity as well in that some tickets on Pastel also need to be *activated* in addition to being registered, which generally happens a few blocks after the registration is completed, but we can gloss over that for now without loss of generality.

Now, how does the selected Bridge node contributor actually get the input data from the user? For example, how does the user transmit the file they want to store to the Bridge node contributor? There are obviously a few ways this could be implemented. For one thing, the user could get the IP address of the selected Bridge node contributor from the Bridge program and simply call a REST API on the Bridge node to transfer the file. But to keep things more reliable, secure, easily monitored, and asynchronous, we’ve elected to use IPFS for this transfer. Basically, the end user can simply submit their file to IPFS and pin it just for the time period it takes for the Bridge node to retrieve it. This way, the end user can just include the IPFS identifier in the service request and doesn’t have to interface directly with the Bridge node, and thus doesn’t have to reveal its IP address. However, we can also provide an optional REST option as well to make it simpler to implement in the end user’s client/browser.  

Once the Bridge node contributor finished executing the service request on behalf of the end user, they will get back the Pastel TXID that represents the completed ticket that serves as proof that the request was completed; that is, once that corresponding Pastel transaction is mined and included in a valid Pastel block. The Bridge node contributor itself causes the TXID to be tracked by the Oracle program by submitting this TXID to the Oracle program along with a small SOL denominated fee to defray to cost of running the Oracle contract (these fees are mostly used as rewards to Oracle contributor nodes, which is the reason they are incentivized to run an Oracle node in the first place).

In any case, once the Oracle program determines that the service request TXID was mined correctly and corresponds to a certain file hash, the Bridge node can observe this on Solana and then cause the escrowed fee from the user to be transferred to the selected Bridge node contributor, minus a a small amount of SOL to be paid into the Bridge program’s reward pool account, where it is used to defray the operating costs of running the Bridge program on Solana. You can review the complete Rust Anchor code for the Bridge program [here](https://github.com/pastelnetwork/solana_pastel_bridge_program/blob/master/programs/solana_pastel_bridge_program/src/lib.rs), but keep in mind that we are still finishing the complete end-to-end typescript testing script, and it likely has some issues still that will need to be fixed. The Rust code does compile in anchor now without any warnings or errors though.

This now brings us to the Oracle Program.

## The Oracle Program

You can think of the Oracle program as essentially being like a mini “Chainlink” just for linking transaction information from Pastel’s blockchain to Solana. It is what allows users on Solana, including the Bridge program itself, to reliably know the status and key contents of any tracked transaction on Pastel based on the TXID of the transaction on the Pastel blockchain.

Similar to the Bridge program, anyone who wants to can set up their own node as an Oracle contributor; the requirements are precisely the same as those for setting up a Bridge node contributor— a Pastel full node, a Pastel Walletnode, a PastelID, and some way of interfacing with Solana. Just like with the Bridge program, a potential contributor would register as a new Oracle contributor by submitting a modest non-refundable entrance fee to the Oracle program. This fee is paid into the Oracle’s reward pool and can then be by the Oracle program to pay fees to Oracle contributors (that is, it is an additional source of the fees, on top of the fees that Bridge nodes pay to the Oracle program to pay for the cost of adding a Pastel TXID to be tracked by the Oracle program).

The Oracle program tracks registered contributors in a similar manner to how the Bridge program tracks Bridge node contributors, with this struct:

```rust
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
```

Now, supposed that we already have a bunch of different independently run Oracle contributor nodes, and suppose that a Bridge node has just submitted a new TXID to the Oracle program for monitoring; the status of this TXID is tracked within the Oracle program by means of this struct:

```rust
    pub struct CommonReportData {
        pub txid: String,
        pub txid_status: TxidStatus,
        pub pastel_ticket_type: Option<PastelTicketType>,
        pub first_6_characters_of_sha3_256_hash_of_corresponding_file: Option<String>,
    }
```

where we have:

```rust
    pub enum TxidStatus {
        Invalid,
        PendingMining,
        MinedPendingActivation,
        MinedActivated,
    }
```

and

```rust
    pub enum PastelTicketType {
        Sense,
        Cascade,
        Nft,
        InferenceApi,
    }
```

And before even allowing this newly submitted TXID to be tracked by the Oracle program, the Oracle confirms that the submitter of the new TXID (which can be anyone without requiring any prior registration, since all they can do is ask for a particular TXID to be tracked) has paid the required Oracle fee, which it tracks using these structs:

```rust
    pub struct PendingPayment {
        pub txid: String,
        pub expected_amount: u64,
        pub payment_status: PaymentStatus,
    }
    
    pub enum PaymentStatus {
        Pending,
        Received,
    }
```

Once this is confirmed to be paid in the correct amount on Solana, the Oracle program lets all the Oracle contributor nodes know that they have a new Pastel TXID to monitor. They grab the TXID string from the Oracle program via Solana and then simply check the status of this TXID on Pastel by using their Pastel full node. They can then submit the status of that TXID to the Oracle program by calling the `submit_data_report`  method of the Oracle program and supplying a status report which identifies the Oracle contributor, the TXID, and other important information:

```rust
    pub struct PastelTxStatusReport {
        pub txid: String,
        pub txid_status: TxidStatus,
        pub pastel_ticket_type: Option<PastelTicketType>,
        pub first_6_characters_of_sha3_256_hash_of_corresponding_file: Option<String>,
        pub timestamp: u64,
        pub contributor_reward_address: Pubkey,
    }
```

The Oracle program collects all these submitted reports in a highly space-optimized way using Solana PDAs and aggregates them together using a sophisticated algorithm to determine a consensus view of the status of the TXID according to the whole group of Oracle contributors that have submitted status reports, where each report is weighted by the reputation score of the specific Oracle contributor. These reputation scores are essentially driven by how much that Oracle contributor’s status reports have matched the eventual consensus view. That is, if that Contributor always reports a status for a given TXID that matches what the rest of the group agrees on, that enhances that Contributor’s reputation score, giving more weight to its status reports in the future. The actual scoring system is far more involved though, and includes bonuses for consistency and completeness.

To minimize space, we only store certain “common” data for a TXID once using this struct:

```rust
    pub struct CommonReportData {
        pub txid: String,
        pub txid_status: TxidStatus,
        pub pastel_ticket_type: Option<PastelTicketType>,
        pub first_6_characters_of_sha3_256_hash_of_corresponding_file: Option<String>,
    }
```

and then we can update the consensus view by using smaller structs that reference this common data (this is more of an internal implementation detail, but shown here just to show how we’ve tried hard to make things as efficient as possible):

```rust
    pub struct SpecificReportData {
        pub contributor_reward_address: Pubkey,
        pub timestamp: u64,
        pub common_data_ref: u64, // Reference to CommonReportData
    }
    
    pub struct TempTxStatusReport {
        pub common_data_ref: u64, // Index to CommonReportData in common_reports
        pub specific_data: SpecificReportData,
    } 
```

**Note: the following section goes into more concrete details about his Oracle tasks are actually implemented in the code; you can skip over this part if you just want to understand the basic overall flow of the system.**

The process for determining the final consensus uses these two functions:

```rust
    fn get_aggregated_data<'a>(
        aggregated_data_account: &'a Account<AggregatedConsensusDataAccount>,
        txid: &str,
    ) -> Option<&'a AggregatedConsensusData> {
        aggregated_data_account
            .consensus_data
            .iter()
            .find(|data| data.txid == txid)
    }
    
    fn compute_consensus(aggregated_data: &AggregatedConsensusData) -> (TxidStatus, String) {
        let consensus_status = aggregated_data
            .status_weights
            .iter()
            .enumerate()
            .max_by_key(|&(_, weight)| weight)
            .map(|(index, _)| usize_to_txid_status(index).unwrap_or(TxidStatus::Invalid))
            .unwrap();
    
        let consensus_hash = aggregated_data
            .hash_weights
            .iter()
            .max_by_key(|hash_weight| hash_weight.weight)
            .map(|hash_weight| hash_weight.hash.clone())
            .unwrap_or_default();
    
        (consensus_status, consensus_hash)
    }
```

This whole process is controlled by this function:

```rust
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
    
            if common_data.txid == txid
                && !updated_contributors.contains(&specific_data.contributor_reward_address)
            {
                if let Some(contributor) = contributor_data_account
                    .contributors
                    .iter_mut()
                    .find(|c| c.reward_address == specific_data.contributor_reward_address)
                {
                    let is_accurate = common_data.txid_status == consensus_status
                        && common_data
                            .first_6_characters_of_sha3_256_hash_of_corresponding_file
                            .as_ref()
                            .map_or(false, |hash| hash == &consensus_hash);
                    update_contributor(contributor, current_timestamp, is_accurate);
                    updated_contributors.push(specific_data.contributor_reward_address);
                }
                contributor_count += 1;
            }
        }
        msg!("Consensus reached for TXID: {}, Status: {:?}, Hash: {}, Number of Contributors Included: {}", txid, consensus_status, consensus_hash, contributor_count);
    
        Ok(())
    }
```

Once the final consensus of the newly tracked TXID is determined, the Oracle contract can then update all its records to reflect this; that is, it can revise all the Oracle contributor nodes’ reputation scores and ban any contributors for which this score is too low. It does this using these functions:

First, this function controls the process:

```rust
    fn update_contributor(contributor: &mut Contributor, current_timestamp: u64, is_accurate: bool) {
        // Check if the contributor is banned before proceeding. If so, just return.
        if contributor.calculate_is_banned(current_timestamp) {
            msg!(
                "Contributor is currently banned and cannot be updated: {}",
                contributor.reward_address
            );
            return; // We don't stop the process here, just skip this contributor.
        }
    
        // Updating scores
        update_scores(contributor, current_timestamp, is_accurate);
    
        // Applying bans based on report accuracy
        apply_bans(contributor, current_timestamp, is_accurate);
    
        // Updating contributor statuses
        update_statuses(contributor, current_timestamp);
    }
```

This in turn calls this function to calculate the new reputation scores taking various metrics into account:

```rust
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
        let reliability_factor = (contributor.accurate_reports_count as f32
            / contributor.total_reports_submitted as f32)
            .clamp(0.0, 1.0);
        contributor.compliance_score = (contributor.compliance_score * reliability_factor).min(100.0);
    
        contributor.compliance_score = logistic_scale(contributor.compliance_score, 100.0, 0.1, 50.0); // Adjusted logistic scaling
    
        contributor.reliability_score = reliability_factor * 100.0;
    
        log_score_updates(contributor);
    }
    
    fn logistic_scale(score: f32, max_value: f32, steepness: f32, midpoint: f32) -> f32 {
        max_value / (1.0 + (-steepness * (score - midpoint)).exp())
    }
```

and this applies the bans:

```rust
    fn apply_bans(contributor: &mut Contributor, current_timestamp: u64, is_accurate: bool) {
        if !is_accurate {
            if contributor.total_reports_submitted <= CONTRIBUTIONS_FOR_TEMPORARY_BAN
                && contributor.consensus_failures % TEMPORARY_BAN_THRESHOLD == 0
            {
                contributor.ban_expiry = current_timestamp + TEMPORARY_BAN_DURATION;
                msg!("Contributor: {} is temporarily banned as of {} because they have submitted {} reports and have {} consensus failures, more than the maximum allowed consensus failures of {}. Ban expires on: {}", 
                contributor.reward_address, current_timestamp, contributor.total_reports_submitted, contributor.consensus_failures, TEMPORARY_BAN_THRESHOLD, contributor.ban_expiry);
            } else if contributor.total_reports_submitted >= CONTRIBUTIONS_FOR_PERMANENT_BAN
                && contributor.consensus_failures >= PERMANENT_BAN_THRESHOLD
            {
                contributor.ban_expiry = u64::MAX;
                msg!("Contributor: {} is permanently banned as of {} because they have submitted {} reports and have {} consensus failures, more than the maximum allowed consensus failures of {}. Removing from list of contributors!", 
                contributor.reward_address, current_timestamp, contributor.total_reports_submitted, contributor.consensus_failures, PERMANENT_BAN_THRESHOLD);
            }
        }
    }
```

There are actually two kinds of bans, temporary and permanent; the permanent bans are reserved for more serious infractions by Oracle contributors, such as repeated failure to confirm to the consensus view, whereas temporary bans could result from more innocent failures by Oracle contributors. The function for applying the permanent bans looks like this:

```rust
    pub fn apply_permanent_bans(contributor_data_account: &mut Account<ContributorDataAccount>) {
        // Collect addresses of contributors to be removed for efficient logging
        let contributors_to_remove: Vec<String> = contributor_data_account
            .contributors
            .iter()
            .filter(|c| c.ban_expiry == u64::MAX)
            .map(|c| c.reward_address.to_string()) // Convert Pubkey to String
            .collect();
    
        // Log information about the removal process
        msg!("Now removing permanently banned contributors! Total number of contributors before removal: {}, Number of contributors to be removed: {}, Addresses of contributors to be removed: {:?}",
            contributor_data_account.contributors.len(), contributors_to_remove.len(), contributors_to_remove);
    
        // Retain only contributors who are not permanently banned
        contributor_data_account
            .contributors
            .retain(|c| c.ban_expiry != u64::MAX);
    }
```

There is a complete, 11 part end-to-end typescript testing script for the Oracle contract [here](https://github.com/pastelnetwork/solana_pastel_oracle_program/blob/master/tests/solana_pastel_oracle_program.ts), which is a good way to see the entire flow of the Oracle contract and how it's supposed to work in practice. The complete Anchor Rust code for the Oracle program is [here](https://github.com/pastelnetwork/solana_pastel_oracle_program/blob/master/programs/solana_pastel_oracle_program/src/lib.rs). The program compiles and all tests pass now using the latest version of Solana and Anchor. However, we are in the process now of further optimizing the code and making it more efficient and more closely follow accepted best practices for Anchor, so there may be some changes to the code in the future.

In any case, all of this results in the Oracle program reaching a reliable conclusion about the status of a given Pastel ID and making this status information available to anyone on Solana. Because this information is “crowd sourced” in a reliable, decentralized, robust way that takes past accuracy into account and bans misbehaving contributors, it can be deemed as an accurate source of ground truth for what’s happening on Pastel Network without having to trust any person or group of people who might have a financial incentive to lie (for example, a Bridge node claiming falsely that it did complete a service request for a user without actually doing it, and thereby tricking the Bridge program into paying it the end user’s SOL fee despite not doing what the end user agreed to).

## Wrapping it Up

As soon as a service request is submitted to the Bridge program by an end user and the end user pays the quoted fee into escrow in the Bridge program, it starts a clock where the selected Bridge node contributor must complete the service request for the user and prove that this has been done by adding the TXID to the Oracle and then having the Oracle report the TXID as being completed successfully. The Bridge program can then verify this by observing the Oracle program on Solana, and can further compare the reported hash of the file to the hash specified by the end user in the original service request. This prevents the selected Bridge node contributor from, say, swapping out a 100mb file for some other 100kb file and trying to claim that it was successfully stored in Cascade. The Bridge program would be able to see through any such deception and would only release the escrowed end user fee to the Bridge node contributor if everything checks out.

What if the selected Bridge node contributor doesn’t follow through? Either through malfeasance or from just bad luck (say, they lost network connectivity, or there was some other problem that prevented their good faith efforts from working out correctly in time), not every service request is going to work out. In that case, the Bridge program simply refunds the full amount of the SOL fee back to the end user, and the end user has only wasted some time. The Bridge node’s reputation score would go down to reflect their failure to complete their assigned task, and if that score gets too low, they will be banned from acting as a Bridge node contributor either temporarily or permanently. The end user is then free to submit the same request again to the Bridge program if they want to try it again, and a different Bridge contributor can be selected to complete the request.

In this design, the Bridge node contributor is taking on some risk to itself in that it’s possible that it will spend the PSL to perform the end user’s service request, but for some reason that service request might not go through successfully on Pastel, either because of some mistake or technical glitch that the Bridge node contributor is responsible for, or even worse, if for some reason the Pastel blockchain becomes unavailable after it has just agreed to perform a service request (say, some kind of blockchain fork, or extreme DDOS attack, etc). In that case, the Bridge node contributor could be out of luck. But over time, they stand to make profit by facilitating user service requests, especially because they are free to set their own prices and only act when the profits are high enough to compensate for any perceived risks they are taking. Also, Bridge nodes can take obvious safeguards, like not bidding on any service requests if they notice that they are unable to access Pastel data or Solana data properly at the moment, or if they have some other technical problem (for example, if their PSL holdings in their Pastel wallet have been exhausted so that they would be unable to pay for the service request on their end).

Another nice advantage of this design/structure is that it completely avoids legal/regulatory issues associated with converting one crypto into another; i.e., SOL to PSL. Whereas a bridging/swapping system that literally converts SOL into PSL for use on Pastel might qualify as a money changing business in some jurisdictions, in our design, the end user is simply paying a SOL fee for a service; i.e., to have a file stored for them on Pastel. How that service is ultimately paid for on Pastel using PSL is outside the scope of the end user’s interaction with the Bridge program. From their perspective, they are simply paying an amount of SOL and getting an action performed. The Bridge node contributor is thus acting like the middle-man and taking on any risk financial risk; they are the one that is dealing with all the messy complexity of setting up the Pastel software and interacting with the Pastel Network, leaving the end user to have a very simple, safe, and fully Solana-native experience.

So what else is left to do? As of 7/12/2024, we just need to finish the testing code for the Bridge and make any required revisions to the Rust/Anchor Bridge code, and also ensure that the Bridge program can successfully integrate with the Oracle program to monitor the status of any tracked Pastel TXIDs. We also need to implement the Python code that manages the process for Bridge and Oracle contributors and which will interact with Solana and with Pastel. But this is all very straightforward and should be completed in under a week, since the bulk of this functionality already exists for other purposes (i.e,. we have have [this](https://github.com/pastelnetwork/opennode_fastapi) Python project that we can start from for interacting with Pastel's daemon and Walletnode), and the code to simply check data from Solana programs and call methods on them is very simple to write. We have held off on writing this code for now simply because we want to finalize everything with the two Solana programs (something that we have less experience with and thus which has taken us longer to do) so we don't have to re-write any Python code that interacts with them.

Once this is all done, we will have a fully functioning system that allows users on Solana to interact with Pastel Network in a seamless, secure, and reliable way, without having to worry about the complexities of dealing with a second blockchain or a second set of tokens. We will also have a system that allows for a decentralized, reliable, and robust way to verify the status of any tracked Pastel TXID on Solana, which can be used for a wide variety of purposes beyond just the Bridge program. We are very excited to see this system in action and to see how it can be used to facilitate a wide variety of interactions between Solana and Pastel, and to see how it can be used to create new and innovative applications that leverage the strengths of both blockchains. We believe that this system will open up the much larger Solana ecosystem to Pastel, which has up til now been more isolated from the broader crypto world, and will allow for a wide variety of new and exciting applications to be built that leverage the strengths of both blockchains. At the same time, we also think Solana users can benefit from leveraging what we think is the most robust, decentralized, and censorship-resistant file storage layer in the world in the form of Cascade, not to mention the near-duplicate detection and image fingerprinting capabilities of Sense, which can be easily leveraged by the Solana NFT ecosystem for new projects to add additional security and value to their NFTs.

Finally, we are most excited about Pastel's new Inference Layer, which allows users to do all sorts of AI tasks in a completely decentralized way, where they don't have to use any centralized API, don't need to provide an email address, Google account, or credit card, but can instead remain totally pseudonymous and pay for services using crypto in a trustless way. This also opens up the power of AI models for use by other crypto projects which can't use centralized APIs like OpenAI's or Anthropic's because there is no way to do so without introducing fundamental centralization in the form of API keys, billing information, etc. Users will be able to leverage these various services, both offerings from OpenAI, Anthropic, Mistral, etc. via Pastel Supernodes (the independent operators of Pastel Supernodes can, at their own option, supply API keys to expose these service offerings to users in a decentralized way, but they don't have to), as well as local LLMs that can be hosted directly on the Supernodes themselves or by connecting to a shared GPU enabled machine controlled by the Supernode operator and shares across a group of their own personal Supernodes. This is all enabled using the popular [Swiss Army Llama](https://github.com/Dicklesworthstone/swiss_army_llama) project created and maintained by Pastel's CEO, Jeff Emanuel. This allows Supernodes to offer completely uncensored, powerful models, such as [this](https://huggingface.co/Orenguteng/Llama-3-8B-Lexi-Uncensored-GGUF) uncensored Llama3 model that won't refuse *any* request out of "AI safety concerns" like the centralized services such as OpenAI do (seriously— it will even tell you in detail how to make meth at home, or how to hide a body— not that we condone such uses!).

Why is this important? Because it finally allows both end users and other crypto projects to tap into the huge power of these new models in an efficient, cost-effective way that is completely uncensored, trustless, and decentralized. For example, a Solana crypto project for a prediction market similar to Polymarket could enforce moderation rules at the time of creation by an end user of a new prediction event by using a Pastel Supernode to check the event description against a model like Llama3 to ensure that it doesn't violate the law or community standards (for example, by creating an "assassination market" that predicts the death of a famous person before a certain future date). Such enforcement is virtually impossible to do algorithmically using traditional programming methods, but is perfect for a powerful LLM such as Llama3 given a detailed prompt that explains what is and is not allowed on the platform. That way, the project can reject unacceptable prediction events at the time of creation, rather than having to wait for a user to report the event after it has already been created and potentially caused harm; and even in that scenario,they would still need to have some kind of ex-post moderation power, which introduces human judgement/bias and censorship into what could otherwise be a completely automated, algorithmic, and impartial process. The ability for such powerful functionality to be called in a completely native Solana way by end users and projects, paying for services in SOL, and having them executed reliably and efficiently by Pastel Supernodes, unlocks so many new use cases and functionalities that were previously impossible to implement in a trustless, decentralized, and efficient way. This is just one example of the many ways that the Inference Layer can be used to add value to the Solana ecosystem, and we are excited to see how it will be used in practice.
