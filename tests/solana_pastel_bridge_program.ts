import * as anchor from "@coral-xyz/anchor";
import { Program, web3, AnchorProvider, BN } from "@coral-xyz/anchor";
import {
  SolanaPastelBridgeProgram,
  IDL,
} from "../target/types/solana_pastel_bridge_program";
import { assert, expect } from "chai";
import * as crypto from "crypto";
const { ComputeBudgetProgram, Transaction, PublicKey } = anchor.web3;

process.env.ANCHOR_PROVIDER_URL = "http://127.0.0.1:8899";
process.env.RUST_LOG =
  "solana_runtime::system_instruction_processor=trace,solana_runtime::message_processor=trace,solana_bpf_loader=debug,solana_rbpf=debug";
const provider = AnchorProvider.env();
anchor.setProvider(provider);
const programID = new anchor.web3.PublicKey(
  "Ew8ohkPJ3JnWoZ3MWvkn86wYMRJkS385Bsis9TwQJo79"
);
const program = new Program<SolanaPastelBridgeProgram>(
  IDL,
  programID,
  provider
);
const admin = provider.wallet; // Use the provider's wallet
const bridgeContractState = web3.Keypair.generate();
let bridgeNodes: web3.Keypair[] = []; // Array to store bridge node keypairs
let trackedTxids: string[] = []; // Initialize an empty array to track TXIDs

const maxSize = 100 * 1024; // 100KB (max size of the bridge contract state account)

const NUM_BRIDGE_NODES = 12;
const NUMBER_OF_SIMULATED_SERVICE_REQUESTS = 20;

const REGISTRATION_ENTRANCE_FEE_SOL = 0.1;
const COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING = 0.0001;
const MIN_NUMBER_OF_ORACLES = 8;
const MIN_REPORTS_FOR_REWARD = 10;
const BAD_BRIDGE_NODE_INDEX = 5; // Define a constant to represent the index at which bridge nodes start submitting incorrect reports with increasing probability
const MIN_COMPLIANCE_SCORE_FOR_REWARD = 65;
const MIN_RELIABILITY_SCORE_FOR_REWARD = 80;
const BASE_REWARD_AMOUNT_IN_LAMPORTS = 100000;

const ErrorCodeMap = {
  0x0: 'BridgeNodeAlreadyRegistered',
  0x1: 'UnregisteredBridgeNode',
  0x2: 'InvalidServiceRequest',
  0x3: 'EscrowNotFunded',
  0x4: 'InvalidFileSize',
  0x5: 'InvalidServiceType',
  0x6: 'QuoteExpired',
  0x7: 'BridgeNodeInactive',
  0x8: 'InsufficientEscrowFunds',
  0x9: 'DuplicateServiceRequestId',
  0xa: 'ContractPaused',
  0xb: 'InvalidFileHash',
  0xc: 'InvalidIpfsCid',
  0xd: 'InvalidUserSolAddress',
  0xe: 'InvalidRequestStatus',
  0xf: 'InvalidPaymentInEscrow',
  0x10: 'InvalidInitialFieldValues',
  0x11: 'InvalidEscrowOrFeeAmounts',
  0x12: 'InvalidServiceRequestId',
  0x13: 'InvalidPastelTxid',
  0x14: 'BridgeNodeNotSelected',
  0x15: 'UnauthorizedBridgeNode',
  0x16: 'ContractNotInitialized',
  0x17: 'MappingNotFound',
  0x18: 'TxidMismatch',
  0x19: 'OutdatedConsensusData',
  0x1a: 'ServiceRequestNotFound',
  0x1b: 'ServiceRequestNotPending',
  0x1c: 'QuoteResponseTimeExceeded',
  0x1d: 'BridgeNodeBanned',
  0x1e: 'InvalidQuotedPrice',
  0x1f: 'InvalidQuoteStatus',
  0x20: 'BridgeNodeScoreTooLow',
  0x21: 'TxidNotFound',
  0x22: 'TxidMappingNotFound',
  0x23: 'InsufficientRegistrationFee',
  0x24: 'UnauthorizedWithdrawalAccount',
  0x25: 'InsufficientFunds',
  0x26: 'TimestampConversionError'
};

const TxidStatusEnum = {
  Invalid: "Invalid",
  PendingMining: "PendingMining",
  MinedPendingActivation: "MinedPendingActivation",
  MinedActivated: "MinedActivated",
};

const PastelTicketTypeEnum = {
  Sense: "Sense",
  Cascade: "Cascade",
  Nft: "Nft",
  InferenceApi: "InferenceApi",
};

console.log("Program ID:", programID.toString());
console.log("Admin ID:", admin.publicKey.toString());

describe("Solana Pastel Bridge Program Tests", () => {
  // Initialize the bridge contract state
  it("Initializes and expands the bridge contract state", async () => {
    // Find the PDAs for the RewardPoolAccount and FeeReceivingContractAccount
    const [rewardPoolAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("bridge_reward_pool_account")],
      program.programId
    );
    const [feeReceivingContractAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_escrow_account")],
        program.programId
      );

    // Find the PDA for the BridgeNodeDataAccount
    const [bridgeNodeDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_nodes_data")],
        program.programId
      );

    // Find the PDA for the ServiceRequestTxidMappingDataAccount
    const [serviceRequestTxidMappingDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("service_request_txid_mapping_data")],
        program.programId
      );

    // Find the PDA for the AggregatedConsensusDataAccount
    const [aggregatedConsensusDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("aggregated_consensus_data")],
        program.programId
      );

    // Find the PDA for the TempServiceRequestsDataAccount
    const [tempServiceRequestsDataAccountPDA] = web3.PublicKey.findProgramAddressSync(
      [Buffer.from("temp_service_requests_data")],
      program.programId
    );

    // Calculate the rent-exempt minimum balance for the account size
    const minBalanceForRentExemption =
      await provider.connection.getMinimumBalanceForRentExemption(100 * 1024); // 100KB
    console.log(
      "Minimum Balance for Rent Exemption:",
      minBalanceForRentExemption
    );

    // Fund the bridgeContractState account with enough SOL for rent exemption
    console.log("Funding Bridge Contract State account for rent exemption");
    const fundTx = new anchor.web3.Transaction().add(
      anchor.web3.SystemProgram.transfer({
        fromPubkey: admin.publicKey,
        toPubkey: bridgeContractState.publicKey,
        lamports: minBalanceForRentExemption,
      })
    );
    await provider.sendAndConfirm(fundTx);

    // Initial Initialization
    console.log("Initializing Bridge Contract State");
    await program.methods
      .initialize(admin.publicKey)
      .accounts({
        bridgeContractState: bridgeContractState.publicKey,
        bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
        user: admin.publicKey,
        bridgeRewardPoolAccount: rewardPoolAccountPDA,
        bridgeEscrowAccount: feeReceivingContractAccountPDA,
        tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
        serviceRequestTxidMappingDataAccount: serviceRequestTxidMappingDataAccountPDA,
        aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([bridgeContractState])
      .rpc();

    let state = await program.account.bridgeContractState.fetch(
      bridgeContractState.publicKey
    );
    assert.ok(
      state.isInitialized,
      "Bridge Contract State should be initialized after first init"
    );
    assert.equal(
      state.adminPubkey.toString(),
      admin.publicKey.toString(),
      "Admin public key should match after first init"
    );

    // Incremental Reallocation
    let currentSize = 10_240; // Initial size after first init

    while (currentSize < maxSize) {
      console.log(
        `Expanding Bridge Contract State size from ${currentSize} to ${
          currentSize + 10_240
        }`
      );
      await program.methods
        .reallocateBridgeState()
        .accounts({
          bridgeContractState: bridgeContractState.publicKey,
          adminPubkey: admin.publicKey,
          tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
          bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
          serviceRequestTxidMappingDataAccount: serviceRequestTxidMappingDataAccountPDA,
          aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
        })
        .rpc();

      currentSize += 10_240;
      state = await program.account.bridgeContractState.fetch(
        bridgeContractState.publicKey
      );

      // Log the updated size of the account
      console.log(`Bridge Contract State size after expansion: ${currentSize}`);
    }

    // Final Assertions
    assert.equal(
      currentSize,
      maxSize,
      "Bridge Contract State should reach the maximum size"
    );
    console.log(
      "Bridge Contract State expanded to the maximum size successfully"
    );
  });

  // Bridge Node Registration
  it("Registers new bridge nodes", async () => {
    // Find the PDAs for the RewardPoolAccount and FeeReceivingContractAccount
    const [rewardPoolAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("bridge_reward_pool_account")],
      program.programId
    );
    const [feeReceivingContractAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_escrow_account")],
        program.programId
      );

    const [bridgeNodeDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_nodes_data")],
        program.programId
      );

    for (let i = 0; i < NUM_BRIDGE_NODES; i++) {
      // Generate a new keypair for each bridge node
      const bridgeNode = web3.Keypair.generate();

      // Transfer the registration fee to feeReceivingContractAccount PDA
      const transaction = new web3.Transaction().add(
        web3.SystemProgram.transfer({
          fromPubkey: admin.publicKey,
          toPubkey: feeReceivingContractAccountPDA,
          lamports: REGISTRATION_ENTRANCE_FEE_SOL * web3.LAMPORTS_PER_SOL,
        })
      );

      // Sign and send the transaction
      await provider.sendAndConfirm(transaction);


      const someValidPastelId = "jXY2fNFPa8sZKkVJNSFYhpEFN5fw9oFRqDAuDS2tLkVi1WwFUqrYxUnn3kCXJMp7u9VRkPNS4aCPDut3vS9PTi";
      const bridgeNodePslAddress = "PtfvkyArQ4nAawzzv4hpSrgCbN6kVkJ9xda";
      // Call the RPC method to register the new bridge node
      await program.methods
        .registerNewBridgeNode(someValidPastelId, bridgeNodePslAddress)
        .accounts({
          bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
          bridgeRewardPoolAccount: rewardPoolAccountPDA,
          user: bridgeNode.publicKey,
        })
        .signers([bridgeNode])
        .rpc();

      console.log(
        `Bridge Node ${i + 1} registered successfully with the address:`,
        bridgeNode.publicKey.toBase58()
      );
      bridgeNodes.push(bridgeNode);
    }

    // Fetch the BridgeNodeDataAccount to verify all bridge nodes are registered
    const bridgeNodeData = await program.account.bridgeNodeDataAccount.fetch(
      bridgeNodeDataAccountPDA
    );
    console.log(
      "Total number of registered bridge nodes in BridgeNodeDataAccount:",
      bridgeNodeData.bridgeNodes.length
    );

    // Verify each bridge node is registered in BridgeNodeDataAccount
    bridgeNodes.forEach((bridgeNode, index) => {
      const isRegistered = bridgeNodeData.bridgeNodes.some((bn) =>
        bn.rewardAddress.equals(bridgeNode.publicKey)
      );
      assert.isTrue(
        isRegistered,
        `Bridge Node ${
          index + 1
        } should be registered in BridgeNodeDataAccount`
      );
    });
  });

  // Define the serviceRequestIds array before it's used
  const serviceRequestIds: string[] = [];

  // Submit Service Requests
  it("Submits service requests", async () => {
    // Find the PDA for the TempServiceRequestsDataAccount
    const [tempServiceRequestsDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("temp_service_requests_data")],
      program.programId
    );

    // Find the PDA for the ServiceRequestSubmissionAccount
    const serviceRequestIds = []; // Array to store service request IDs for later use

    for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
      const serviceRequestSubmissionAccount = web3.Keypair.generate();
      const fileHash = crypto.createHash('sha3-256').update(`file${i}`).digest('hex').substring(0, 6);
      const pastelTicketTypeString = Object.keys(PastelTicketTypeEnum)[i % Object.keys(PastelTicketTypeEnum).length];
      const ipfsCid = `Qm${crypto.randomBytes(44).toString('hex')}`;
      const fileSizeBytes = Math.floor(Math.random() * 1000000) + 1; // Random file size between 1 and 1,000,000 bytes

      const serviceRequestId = crypto.createHash('sha256').update(pastelTicketTypeString + fileHash + admin.publicKey.toString()).digest('hex').substring(0, 32);
      serviceRequestIds.push(serviceRequestId);

      // Submit the service request
      await program.methods
        .submitServiceRequest(pastelTicketTypeString, fileHash, ipfsCid, new BN(fileSizeBytes))
        .accounts({
          serviceRequestSubmissionAccount: serviceRequestSubmissionAccount.publicKey,
          bridgeContractState: bridgeContractState.publicKey,
          tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
          user: admin.publicKey,
          systemProgram: web3.SystemProgram.programId,
        })
        .signers([serviceRequestSubmissionAccount])
        .rpc();

      console.log(`Service request ${i + 1} submitted successfully with ID: ${serviceRequestId}`);
    }

    // Fetch the TempServiceRequestsDataAccount to verify all service requests are submitted
    const tempServiceRequestsData = await program.account.tempServiceRequestsDataAccount.fetch(
      tempServiceRequestsDataAccountPDA
    );
    console.log(
      "Total number of submitted service requests in TempServiceRequestsDataAccount:",
      tempServiceRequestsData.serviceRequests.length
    );

    // Verify each service request is submitted in TempServiceRequestsDataAccount
    serviceRequestIds.forEach((serviceRequestId, index) => {
      const isSubmitted = tempServiceRequestsData.serviceRequests.some((sr) =>
        sr.serviceRequestId === serviceRequestId
      );
      assert.isTrue(
        isSubmitted,
        `Service Request ${
          index + 1
        } should be submitted in TempServiceRequestsDataAccount`
      );
    });
  });

  // Submit Price Quotes
  it("Submits price quotes for service requests", async () => {
    // Find the PDA for the TempServiceRequestsDataAccount
    const [tempServiceRequestsDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("temp_service_requests_data")],
      program.programId
    );

    // Find the PDA for the BestPriceQuoteReceivedForServiceRequest
    const [bestPriceQuoteAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("best_price_quote_account")],
      program.programId
    );

    for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
      const serviceRequestId = serviceRequestIds[i];
      const bridgeNode = bridgeNodes[i % bridgeNodes.length];
      const quotedPriceLamports = new BN(Math.floor(Math.random() * 1000000) + 1); // Random price between 1 and 1,000,000 lamports

      // Submit the price quote
      await program.methods
        .submitPriceQuote(bridgeNode.publicKey.toString(), serviceRequestId, quotedPriceLamports)
        .accounts({
          priceQuoteSubmissionAccount: web3.Keypair.generate().publicKey,
          bridgeContractState: bridgeContractState.publicKey,
          tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
          user: bridgeNode.publicKey,
          bridgeNodesDataAccount: web3.Keypair.generate().publicKey,
          bestPriceQuoteAccount: bestPriceQuoteAccountPDA,
          systemProgram: web3.SystemProgram.programId,
        })
        .signers([bridgeNode])
        .rpc();

      console.log(`Price quote for service request ${i + 1} submitted successfully by bridge node ${bridgeNode.publicKey.toBase58()}`);
    }

    // Fetch the BestPriceQuoteReceivedForServiceRequest to verify the best price quotes are selected
    const bestPriceQuoteData = await program.account.bestPriceQuoteReceivedForServiceRequest.fetch(
      bestPriceQuoteAccountPDA
    );
    console.log(
      "Best price quotes received for service requests:",
      bestPriceQuoteData
    );

    // Verify the best price quotes are selected for each service request
    serviceRequestIds.forEach((serviceRequestId, index) => {
      assert.equal(
        bestPriceQuoteData.serviceRequestId,
        serviceRequestId,
        `Best price quote should be selected for service request ${index + 1}`
      );
    });
  });

  // Submit Pastel TxIDs
  it("Submits Pastel TxIDs for service requests", async () => {
    // Find the PDA for the TempServiceRequestsDataAccount
    const [tempServiceRequestsDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("temp_service_requests_data")],
      program.programId
    );

    // Find the PDA for the ServiceRequestTxidMappingDataAccount
    const [serviceRequestTxidMappingDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("service_request_txid_mapping_data")],
      program.programId
    );

    for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
      const serviceRequestId = serviceRequestIds[i];
      const bridgeNode = bridgeNodes[i % bridgeNodes.length];
      const pastelTxid = crypto.randomBytes(32).toString('hex'); // Generate a random Pastel TxID

      // Submit the Pastel TxID
      await program.methods
        .submitPastelTxid(serviceRequestId, pastelTxid)
        .accounts({
          serviceRequestSubmissionAccount: web3.Keypair.generate().publicKey,
          bridgeContractState: bridgeContractState.publicKey,
          bridgeNodesDataAccount: web3.Keypair.generate().publicKey,
          serviceRequestTxidMappingDataAccount: serviceRequestTxidMappingDataAccountPDA,
          systemProgram: web3.SystemProgram.programId,
        })
        .signers([bridgeNode])
        .rpc();

      console.log(`Pastel TxID for service request ${i + 1} submitted successfully by bridge node ${bridgeNode.publicKey.toBase58()}`);
    }

    // Fetch the ServiceRequestTxidMappingDataAccount to verify the TxIDs are submitted
    const serviceRequestTxidMappingData = await program.account.serviceRequestTxidMappingDataAccount.fetch(
      serviceRequestTxidMappingDataAccountPDA
    );
    console.log(
      "Total number of submitted Pastel TxIDs in ServiceRequestTxidMappingDataAccount:",
      serviceRequestTxidMappingData.mappings.length
    );

    // Verify each Pastel TxID is submitted in ServiceRequestTxidMappingDataAccount
    serviceRequestIds.forEach((serviceRequestId, index) => {
      const isSubmitted = serviceRequestTxidMappingData.mappings.some((mapping) =>
        mapping.serviceRequestId === serviceRequestId
      );
      assert.isTrue(
        isSubmitted,
        `Pastel TxID for Service Request ${
          index + 1
        } should be submitted in ServiceRequestTxidMappingDataAccount`
      );
    });
  });

  const adminKeypair = (provider.wallet as anchor.Wallet).payer;

  // Access Oracle Data
  it("Accesses Oracle data and handles post-transaction tasks", async () => {
    // Find the PDA for the AggregatedConsensusDataAccount
    const [aggregatedConsensusDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("aggregated_consensus_data")],
      program.programId
    );
  
    // Find the PDA for the ServiceRequestTxidMappingDataAccount
    const [serviceRequestTxidMappingDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("service_request_txid_mapping_data")],
      program.programId
    );
  
    // Fetch aggregated consensus data for each service request
    for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
      const serviceRequestId = serviceRequestIds[i];
      const txid = trackedTxids[i];
  
      // Access the Oracle data
      await program.methods
        .processOracleData(txid, serviceRequestId)
        .accounts({
          aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
          serviceRequestTxidMappingDataAccount: serviceRequestTxidMappingDataAccountPDA,
          tempServiceRequestsDataAccount: web3.Keypair.generate().publicKey,
          bridgeContractState: bridgeContractState.publicKey,
          bridgeNodesDataAccount: web3.Keypair.generate().publicKey,
          bridgeRewardPoolAccount: web3.Keypair.generate().publicKey,
          bridgeEscrowAccount: web3.Keypair.generate().publicKey,
          userAccount: admin.publicKey,
          systemProgram: web3.SystemProgram.programId,
        })
        .signers([adminKeypair]) // Use the payer from the provider's wallet
        .rpc();
  
      console.log(`Oracle data accessed and post-transaction tasks handled for service request ${i + 1} and TxID ${txid}`);
    }
  });
  
});
