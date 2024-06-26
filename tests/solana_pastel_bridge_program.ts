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
      [Buffer.from("reward_pool")],
      program.programId
    );
    const [feeReceivingContractAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("fee_receiving_contract")],
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
      [Buffer.from("reward_pool")],
      program.programId
    );
    const [feeReceivingContractAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("fee_receiving_contract")],
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



});