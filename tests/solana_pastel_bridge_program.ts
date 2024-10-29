import { assert, expect } from "chai";
import Decimal from "decimal.js";
import * as crypto from "crypto";
import * as anchor from "@coral-xyz/anchor";
import { Program, web3, AnchorProvider, BN } from "@coral-xyz/anchor";
import { ComputeBudgetProgram, SystemProgram } from "@solana/web3.js";
import { SolanaPastelBridgeProgram } from "../target/types/solana_pastel_bridge_program";
import IDL from "../target/idl/solana_pastel_bridge_program.json";

const { PublicKey, Keypair, Transaction } = anchor.web3;

// Provider setup
process.env.ANCHOR_PROVIDER_URL = "http://127.0.0.1:8899";
process.env.RUST_LOG =
  "solana_runtime::system_instruction_processor=trace,solana_runtime::message_processor=trace,solana_bpf_loader=debug,solana_rbpf=debug";

const provider = AnchorProvider.env();
anchor.setProvider(provider);

// Let Anchor handle the program ID
const program = new Program<SolanaPastelBridgeProgram>(IDL as any, provider);
const admin = provider.wallet;
const adminPublicKey = admin.publicKey;

// Global state tracking
let bridgeContractState: web3.Keypair;
let bridgeNodes: any[] = [];
let serviceRequestIds: string[] = [];
let totalComputeUnitsUsed = 0;
let maxAccountStorageUsed = 0;

// Configuration constants
const TURN_ON_STORAGE_AND_COMPUTE_PROFILING = true;
const ACCOUNT_DISCRIMINATOR_SIZE = 8;
const MAX_ACCOUNT_SIZE = 10 * 1024; // 10KB max initially
const COMPUTE_UNITS_PER_TX = 1_400_000;
const TX_CONFIRMATION_TIMEOUT = 60000; // 60 seconds
const OPERATION_DELAY = 1000; // 1 second delay between operations

// Account size calculations
const ACCOUNT_SIZES = {
  BRIDGE_NODES: 2048,
  SERVICE_REQUESTS: 2048,
  CONSENSUS_DATA: 2048,
  TXID_MAPPINGS: 2048,
  BASE_STATE: 1024,
};

// Business logic constants
const NUM_BRIDGE_NODES = 3;
const NUMBER_OF_SIMULATED_SERVICE_REQUESTS = 5;
const REGISTRATION_ENTRANCE_FEE_SOL = 0.1;
const COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING = 0.0001;
const MIN_NUMBER_OF_ORACLES = 8;
const MIN_REPORTS_FOR_REWARD = 10;
const BAD_BRIDGE_NODE_INDEX = 5;
const MIN_COMPLIANCE_SCORE_FOR_REWARD = 65;
const MIN_RELIABILITY_SCORE_FOR_REWARD = 80;
const BASE_REWARD_AMOUNT_IN_LAMPORTS = 100000;
const baselinePriceUSD = 3;
const solToUsdRate = 130;
const baselinePriceSol = baselinePriceUSD / solToUsdRate;

// Enums
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

// Helper functions
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const handleProgramError = (error: any, context: string) => {
  console.error(`Error in ${context}:`, error);

  if (error instanceof anchor.AnchorError) {
    console.error(`Error Code: ${error.error.errorCode.code}`);
    console.error(`Error Message: ${error.error.errorMessage}`);
    if (error.error.origin) {
      console.error(`Error Origin: ${error.error.origin}`);
    }
  }

  if (error.logs) {
    console.error("Program logs:", error.logs);
  }

  throw error;
};

const confirmTransaction = async (
  signature: string,
  commitment: web3.Commitment = "confirmed",
  timeout = TX_CONFIRMATION_TIMEOUT
) => {
  const startTime = Date.now();

  try {
    await provider.connection.confirmTransaction(
      { signature, ...(await provider.connection.getLatestBlockhash()) },
      commitment
    );

    if (TURN_ON_STORAGE_AND_COMPUTE_PROFILING) {
      await measureComputeUnitsAndStorage(signature);
    }

    await sleep(OPERATION_DELAY);
  } catch (error) {
    if (Date.now() - startTime > timeout) {
      console.error(`Transaction confirmation timeout after ${timeout}ms`);
      throw new Error(`Transaction confirmation timeout: ${signature}`);
    }
    throw error;
  }
};

const generateRandomPriceQuote = (baselinePriceLamports: number): BN => {
  const variation = Math.floor(Math.random() * (baselinePriceLamports * 0.1));
  const adjustment = Math.random() < 0.5 ? -variation : variation;
  return new BN(baselinePriceLamports + adjustment);
};

const measureComputeUnitsAndStorage = async (txSignature: string) => {
  if (!TURN_ON_STORAGE_AND_COMPUTE_PROFILING) return;

  for (let attempts = 0; attempts < 5; attempts++) {
    try {
      const txDetails = await provider.connection.getParsedTransaction(
        txSignature,
        { commitment: "confirmed" }
      );

      if (txDetails?.meta?.computeUnitsConsumed) {
        totalComputeUnitsUsed += txDetails.meta.computeUnitsConsumed;
      }

      if (txDetails?.transaction.message.accountKeys) {
        for (const accountKey of txDetails.transaction.message.accountKeys) {
          const accountInfo = await provider.connection.getAccountInfo(
            new PublicKey(accountKey.pubkey.toString())
          );
          if (accountInfo && accountInfo.data.length > maxAccountStorageUsed) {
            maxAccountStorageUsed = accountInfo.data.length;
          }
        }
      }
      return;
    } catch (error) {
      if (attempts === 4) {
        console.error(
          `Failed to fetch transaction details for ${txSignature}:`,
          error
        );
      }
      await sleep(250);
    }
  }
};

const calculateSpace = (
  baseSize: number,
  itemSize: number,
  maxItems: number
) => {
  return ACCOUNT_DISCRIMINATOR_SIZE + 4 + (baseSize + itemSize * maxItems);
};

describe("Solana Pastel Bridge Tests", () => {
  before(async () => {
    bridgeContractState = web3.Keypair.generate();
    console.log("Program ID:", program.programId.toString());
    console.log("Admin ID:", adminPublicKey.toString());

    // Fund admin account if needed
    const adminBalance = await provider.connection.getBalance(adminPublicKey);
    if (adminBalance < web3.LAMPORTS_PER_SOL * 100) {
      const airdropSignature = await provider.connection.requestAirdrop(
        adminPublicKey,
        web3.LAMPORTS_PER_SOL * 100
      );
      await confirmTransaction(airdropSignature);
    }
  });

  describe("Initialization", () => {
    it("Initializes and expands the bridge contract state", async () => {
      try {
        // Set up compute budget for all transactions
        const modifyComputeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
          units: 1_400_000,
        });

        const modifyComputePriceIx = ComputeBudgetProgram.setComputeUnitPrice({
          microLamports: 50,
        });

        console.log("Starting PDA generation...");

        // Generate PDAs with proper seeds
        const [rewardPoolAccountPDA] = await PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_reward_pool_account")],
          program.programId
        );

        const [bridgeEscrowAccountPDA] = await PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_escrow_account")],
          program.programId
        );

        const [bridgeNodeDataAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("bridge_nodes_data")],
            program.programId
          );

        const [serviceRequestTxidMappingDataAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("service_request_txid_map")],
            program.programId
          );

        const [aggregatedConsensusDataAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("aggregated_consensus_data")],
            program.programId
          );

        const [tempServiceRequestsDataAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("temp_service_requests_data")],
            program.programId
          );

        const [regFeeReceivingAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("reg_fee_receiving_account")],
            program.programId
          );

        // Log PDA addresses
        console.log({
          rewardPoolAccountPDA: rewardPoolAccountPDA.toBase58(),
          bridgeEscrowAccountPDA: bridgeEscrowAccountPDA.toBase58(),
          bridgeNodeDataAccountPDA: bridgeNodeDataAccountPDA.toBase58(),
          serviceRequestTxidMappingDataAccountPDA:
            serviceRequestTxidMappingDataAccountPDA.toBase58(),
          aggregatedConsensusDataAccountPDA:
            aggregatedConsensusDataAccountPDA.toBase58(),
          tempServiceRequestsDataAccountPDA:
            tempServiceRequestsDataAccountPDA.toBase58(),
          regFeeReceivingAccountPDA: regFeeReceivingAccountPDA.toBase58(),
        });

        // Calculate initial rent exemption for all accounts
        const connection = provider.connection;
        const baseStateSpace =
          8 + // Discriminator
          1 + // is_initialized
          1 + // is_paused
          32 + // admin_pubkey
          32 + // oracle_contract_pubkey
          32 + // bridge_reward_pool_account_pubkey
          32 + // bridge_escrow_account_pubkey
          32 + // bridge_nodes_data_account_pubkey
          32 + // temp_service_requests_data_account_pubkey
          32 + // aggregated_consensus_data_account_pubkey
          32 + // service_request_txid_mapping_account_pubkey
          32; // reg_fee_receiving_account_pubkey

        const rent = await connection.getMinimumBalanceForRentExemption(
          baseStateSpace
        );

        // Fund the bridge contract state account with double the minimum rent
        const fundTx = new web3.Transaction().add(
          web3.SystemProgram.transfer({
            fromPubkey: provider.wallet.publicKey,
            toPubkey: bridgeContractState.publicKey,
            lamports: rent * 2,
          })
        );

        const fundTxSignature = await provider.sendAndConfirm(fundTx);
        await confirmTransaction(fundTxSignature);
        console.log("Bridge contract state funded");

        // Initialize base state
        console.log("Starting base initialization...");
        const initBaseTx = await program.methods
          .initializeBase(provider.wallet.publicKey)
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: provider.wallet.publicKey,
            systemProgram: web3.SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx, modifyComputePriceIx])
          .signers([bridgeContractState])
          .rpc({ skipPreflight: true });

        await confirmTransaction(initBaseTx);
        console.log("Base initialization complete");

        // Add delay between transactions
        await sleep(2000);

        // Initialize core PDAs
        console.log("Starting core PDA initialization...");
        const initCorePDAsTx = await program.methods
          .initializeCorePdas()
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: provider.wallet.publicKey,
            bridgeRewardPoolAccount: rewardPoolAccountPDA,
            bridgeEscrowAccount: bridgeEscrowAccountPDA,
            regFeeReceivingAccount: regFeeReceivingAccountPDA,
            systemProgram: web3.SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx, modifyComputePriceIx])
          .rpc({ skipPreflight: true });

        await confirmTransaction(initCorePDAsTx);
        console.log("Core PDAs initialization complete");

        // Add delay between transactions
        await sleep(2000);

        // Initialize data PDAs
        console.log("Starting data PDA initialization...");
        const initDataPDAsTx = await program.methods
          .initializeDataPdas()
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: provider.wallet.publicKey,
            bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
            tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
            serviceRequestTxidMappingDataAccount:
              serviceRequestTxidMappingDataAccountPDA,
            aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
            systemProgram: web3.SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx, modifyComputePriceIx])
          .rpc({
            skipPreflight: true,
            commitment: "confirmed",
          });

        await confirmTransaction(initDataPDAsTx);
        console.log("Data PDAs initialization complete");

        // Verify all PDAs were initialized correctly
        const accountInfos = await Promise.all([
          connection.getAccountInfo(bridgeNodeDataAccountPDA),
          connection.getAccountInfo(tempServiceRequestsDataAccountPDA),
          connection.getAccountInfo(serviceRequestTxidMappingDataAccountPDA),
          connection.getAccountInfo(aggregatedConsensusDataAccountPDA),
        ]);

        // Verify each account exists and has the correct data size
        accountInfos.forEach((accountInfo, index) => {
          const accountNames = [
            "Bridge Node Data",
            "Temp Service Requests",
            "Service Request Txid Mapping",
            "Aggregated Consensus Data",
          ];

          if (!accountInfo) {
            throw new Error(`${accountNames[index]} account not initialized`);
          }
          console.log(
            `${accountNames[index]} account initialized with ${accountInfo.data.length} bytes`
          );
        });

        // Verify the bridge contract state
        const state = await program.account.bridgeContractState.fetch(
          bridgeContractState.publicKey
        );

        // Perform assertions
        assert.isTrue(
          state.isInitialized,
          "Bridge contract state should be initialized"
        );
        assert.isFalse(
          state.isPaused,
          "Bridge contract should not be paused initially"
        );
        assert.equal(
          state.adminPubkey.toString(),
          provider.wallet.publicKey.toString(),
          "Admin public key should be set correctly"
        );
        assert.equal(
          state.bridgeNodesDataAccountPubkey.toString(),
          bridgeNodeDataAccountPDA.toString(),
          "Bridge nodes data account pubkey should be set correctly"
        );
        assert.equal(
          state.tempServiceRequestsDataAccountPubkey.toString(),
          tempServiceRequestsDataAccountPDA.toString(),
          "Temp service requests data account pubkey should be set correctly"
        );
        assert.equal(
          state.aggregatedConsensusDataAccountPubkey.toString(),
          aggregatedConsensusDataAccountPDA.toString(),
          "Aggregated consensus data account pubkey should be set correctly"
        );
        assert.equal(
          state.serviceRequestTxidMappingAccountPubkey.toString(),
          serviceRequestTxidMappingDataAccountPDA.toString(),
          "Service request txid mapping account pubkey should be set correctly"
        );
        assert.equal(
          state.bridgeRewardPoolAccountPubkey.toString(),
          rewardPoolAccountPDA.toString(),
          "Bridge reward pool account pubkey should be set correctly"
        );
        assert.equal(
          state.bridgeEscrowAccountPubkey.toString(),
          bridgeEscrowAccountPDA.toString(),
          "Bridge escrow account pubkey should be set correctly"
        );
        assert.equal(
          state.regFeeReceivingAccountPubkey.toString(),
          regFeeReceivingAccountPDA.toString(),
          "Registration fee receiving account pubkey should be set correctly"
        );

        // Verify account rent-exemption status
        const accounts = [
          {
            name: "Bridge Contract State",
            pubkey: bridgeContractState.publicKey,
          },
          { name: "Bridge Nodes Data", pubkey: bridgeNodeDataAccountPDA },
          {
            name: "Temp Service Requests",
            pubkey: tempServiceRequestsDataAccountPDA,
          },
          {
            name: "Service Request Txid Mapping",
            pubkey: serviceRequestTxidMappingDataAccountPDA,
          },
          {
            name: "Aggregated Consensus Data",
            pubkey: aggregatedConsensusDataAccountPDA,
          },
          { name: "Bridge Reward Pool", pubkey: rewardPoolAccountPDA },
          { name: "Bridge Escrow", pubkey: bridgeEscrowAccountPDA },
          {
            name: "Registration Fee Receiving",
            pubkey: regFeeReceivingAccountPDA,
          },
        ];

        for (const account of accounts) {
          const accountInfo = await connection.getAccountInfo(account.pubkey);
          if (!accountInfo) {
            throw new Error(`${account.name} account not found`);
          }

          const rent = await connection.getMinimumBalanceForRentExemption(
            accountInfo.data.length
          );
          assert.isTrue(
            accountInfo.lamports >= rent,
            `${account.name} account should be rent-exempt. Required: ${rent}, Current: ${accountInfo.lamports}`
          );
        }

        // Verify data structures initialization
        const bridgeNodesData =
          await program.account.bridgeNodesDataAccount.fetch(
            bridgeNodeDataAccountPDA
          );
        assert.isArray(
          bridgeNodesData.bridgeNodes,
          "Bridge nodes should be initialized as empty array"
        );
        assert.equal(
          bridgeNodesData.bridgeNodes.length,
          0,
          "Bridge nodes array should be empty"
        );

        const tempServiceRequestsData =
          await program.account.tempServiceRequestsDataAccount.fetch(
            tempServiceRequestsDataAccountPDA
          );
        assert.isArray(
          tempServiceRequestsData.serviceRequests,
          "Service requests should be initialized as empty array"
        );
        assert.equal(
          tempServiceRequestsData.serviceRequests.length,
          0,
          "Service requests array should be empty"
        );

        const txidMappingData =
          await program.account.serviceRequestTxidMappingDataAccount.fetch(
            serviceRequestTxidMappingDataAccountPDA
          );
        assert.isArray(
          txidMappingData.mappings,
          "TXID mappings should be initialized as empty array"
        );
        assert.equal(
          txidMappingData.mappings.length,
          0,
          "TXID mappings array should be empty"
        );

        const consensusData =
          await program.account.aggregatedConsensusDataAccount.fetch(
            aggregatedConsensusDataAccountPDA
          );
        assert.isArray(
          consensusData.consensusData,
          "Consensus data should be initialized as empty array"
        );
        assert.equal(
          consensusData.consensusData.length,
          0,
          "Consensus data array should be empty"
        );

        console.log("All initialization verifications completed successfully");
      } catch (error) {
        console.error("Detailed error information:", error);
        if (error.logs) {
          console.error("Program logs:", error.logs);
        }
        throw error;
      }
    });
  });

  describe("Reinitialization Prevention", () => {
    it("Prevents reinitialization of BridgeContractState and PDAs", async () => {
      const modifyComputeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
        units: COMPUTE_UNITS_PER_TX,
      });

      const [rewardPoolAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_reward_pool_account")],
        program.programId
      );
      const [bridgeNodeDataAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_nodes_data")],
        program.programId
      );
      const [tempServiceRequestsDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("temp_service_requests_data")],
          program.programId
        );
      const [aggregatedConsensusDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("aggregated_consensus_data")],
          program.programId
        );

      // Try to reinitialize base state
      try {
        await program.methods
          .initializeBase(adminPublicKey)
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: adminPublicKey,
            systemProgram: SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .rpc();
        assert.fail("Should not be able to reinitialize base state");
      } catch (error) {
        const anchorError = error as anchor.AnchorError;
        assert.include(
          anchorError.error.errorMessage,
          "Bridge Contract state is already initialized"
        );
      }

      // Try to reinitialize PDAs
      try {
        await program.methods
          .initializeDataPdas()
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: adminPublicKey,
            bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
            tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
            serviceRequestTxidMappingDataAccount: bridgeNodeDataAccountPDA,
            aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
            systemProgram: SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .rpc();
        assert.fail("Should not be able to reinitialize PDAs");
      } catch (error) {
        const anchorError = error as anchor.AnchorError;
        assert.include(
          anchorError.error.errorMessage,
          "Bridge Contract state is already initialized"
        );
      }

      // Verify state remains unchanged
      const state = await program.account.bridgeContractState.fetch(
        bridgeContractState.publicKey
      );
      assert.isTrue(
        state.isInitialized,
        "Bridge Contract State should still be initialized"
      );
      assert.equal(
        state.adminPubkey.toString(),
        adminPublicKey.toString(),
        "Admin public key should remain unchanged"
      );
    });
  });

  describe("Bridge Node Registration", () => {
    it("Registers new bridge nodes", async () => {
      const modifyComputeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
        units: COMPUTE_UNITS_PER_TX,
      });

      // Verify account initialization
      const [bridgeNodeDataAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_nodes_data")],
        program.programId
      );

      await sleep(OPERATION_DELAY);

      const accountInfo = await provider.connection.getAccountInfo(
        bridgeNodeDataAccountPDA
      );
      if (!accountInfo) {
        throw new Error("Bridge nodes data account not initialized");
      }

      const [bridgeRewardPoolAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_reward_pool_account")],
          program.programId
        );

      const [regFeeReceivingAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("reg_fee_receiving_account")],
          program.programId
        );

      for (let i = 0; i < NUM_BRIDGE_NODES; i++) {
        const bridgeNode = Keypair.generate();
        console.log(
          `Bridge Node ${i + 1} Keypair:`,
          bridgeNode.publicKey.toBase58()
        );

        try {
          // Fund bridge node
          console.log(`Funding bridge node ${i + 1}...`);
          const fundAmount =
            REGISTRATION_ENTRANCE_FEE_SOL * web3.LAMPORTS_PER_SOL +
            web3.LAMPORTS_PER_SOL; // Extra SOL for transaction fees

          const fundTx = new Transaction().add(
            SystemProgram.transfer({
              fromPubkey: adminPublicKey,
              toPubkey: bridgeNode.publicKey,
              lamports: fundAmount,
            })
          );

          const fundTxSignature = await provider.sendAndConfirm(fundTx);
          await confirmTransaction(fundTxSignature);

          // Generate unique IDs
          const uniquePastelId = crypto.randomBytes(32).toString("hex");
          const uniquePslAddress = "P" + crypto.randomBytes(33).toString("hex");

          // Transfer registration fee
          console.log(
            `Transferring registration fee from bridge node ${i + 1}...`
          );
          const transferTx = new Transaction().add(
            SystemProgram.transfer({
              fromPubkey: bridgeNode.publicKey,
              toPubkey: regFeeReceivingAccountPDA,
              lamports: REGISTRATION_ENTRANCE_FEE_SOL * web3.LAMPORTS_PER_SOL,
            })
          );

          const transferTxSignature = await provider.sendAndConfirm(
            transferTx,
            [bridgeNode]
          );
          await confirmTransaction(transferTxSignature);

          // Register bridge node
          console.log(`Registering bridge node ${i + 1}...`);
          const registerTx = await program.methods
            .registerNewBridgeNode(uniquePastelId, uniquePslAddress)
            .accounts({
              bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
              user: bridgeNode.publicKey,
              bridgeRewardPoolAccount: bridgeRewardPoolAccountPDA,
              regFeeReceivingAccount: regFeeReceivingAccountPDA,
              systemProgram: SystemProgram.programId,
            })
            .preInstructions([modifyComputeBudgetIx])
            .signers([bridgeNode])
            .rpc();

          await confirmTransaction(registerTx);

          console.log(`Bridge Node ${i + 1} registered successfully:`, {
            address: bridgeNode.publicKey.toBase58(),
            pastelId: uniquePastelId,
            pslAddress: uniquePslAddress,
          });

          bridgeNodes.push({
            keypair: bridgeNode,
            pastelId: uniquePastelId,
            pslAddress: uniquePslAddress,
          });

          // Add delay between registrations
          await sleep(OPERATION_DELAY);
        } catch (error) {
          handleProgramError(error, `Bridge Node ${i + 1} Registration`);
        }
      }

      // Verify registrations
      const bridgeNodeData = await program.account.bridgeNodesDataAccount.fetch(
        bridgeNodeDataAccountPDA
      );

      console.log(
        "Total registered bridge nodes:",
        bridgeNodeData.bridgeNodes.length
      );

      bridgeNodes.forEach((bridgeNode, index) => {
        const isRegistered = bridgeNodeData.bridgeNodes.some(
          (bn) =>
            bn.rewardAddress.equals(bridgeNode.keypair.publicKey) &&
            bn.pastelId === bridgeNode.pastelId &&
            bn.bridgeNodePslAddress === bridgeNode.pslAddress
        );

        assert.isTrue(
          isRegistered,
          `Bridge Node ${
            index + 1
          } should be registered in BridgeNodesDataAccount`
        );
      });
    });
  });

  describe("Service Request Handling", () => {
    it("Submits service requests", async () => {
      const modifyComputeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
        units: COMPUTE_UNITS_PER_TX,
      });

      const [tempServiceRequestsDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("temp_service_requests_data")],
          program.programId
        );

      const [aggregatedConsensusDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("aggregated_consensus_data")],
          program.programId
        );

      // Verify account initialization
      const accountInfo = await provider.connection.getAccountInfo(
        tempServiceRequestsDataAccountPDA
      );
      if (!accountInfo) {
        throw new Error("Temp service requests account not initialized");
      }

      const lamports =
        web3.LAMPORTS_PER_SOL *
        COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING;
      const ADDITIONAL_SOL_FOR_ACTUAL_REQUEST = 1;
      const totalFundingLamports =
        lamports + web3.LAMPORTS_PER_SOL * ADDITIONAL_SOL_FOR_ACTUAL_REQUEST;

      for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
        try {
          console.log(
            `Generating service request ${
              i + 1
            } of ${NUMBER_OF_SIMULATED_SERVICE_REQUESTS}`
          );

          // Generate and fund end user account
          const endUserKeypair = web3.Keypair.generate();
          console.log(
            `End user address: ${endUserKeypair.publicKey.toString()}`
          );

          const transferTx = new Transaction().add(
            SystemProgram.transfer({
              fromPubkey: adminPublicKey,
              toPubkey: endUserKeypair.publicKey,
              lamports: totalFundingLamports,
            })
          );

          const transferTxSignature = await provider.sendAndConfirm(transferTx);
          await confirmTransaction(transferTxSignature);

          // Generate request data
          const fileHash = crypto
            .createHash("sha3-256")
            .update(`file${i}`)
            .digest("hex")
            .substring(0, 6);

          const pastelTicketTypeString =
            Object.keys(PastelTicketTypeEnum)[
              i % Object.keys(PastelTicketTypeEnum).length
            ];

          const ipfsCid = `Qm${crypto.randomBytes(44).toString("hex")}`;
          const fileSizeBytes = Math.floor(Math.random() * 1000000) + 1;

          // Generate service request ID
          const concatenatedStr =
            pastelTicketTypeString +
            fileHash +
            endUserKeypair.publicKey.toString();
          const expectedServiceRequestIdHash = crypto
            .createHash("sha256")
            .update(concatenatedStr)
            .digest("hex");
          const expectedServiceRequestId =
            expectedServiceRequestIdHash.substring(0, 24);

          // Derive submission account PDA
          const [serviceRequestSubmissionAccountPDA] =
            await PublicKey.findProgramAddressSync(
              [Buffer.from("srq"), Buffer.from(expectedServiceRequestId)],
              program.programId
            );

          console.log({
            fileHash,
            pastelTicketTypeString,
            ipfsCid,
            fileSizeBytes,
            expectedServiceRequestId,
            submissionAccountPDA: serviceRequestSubmissionAccountPDA.toString(),
          });

          // Submit service request
          const submitTx = await program.methods
            .submitServiceRequest(
              pastelTicketTypeString,
              fileHash,
              ipfsCid,
              new BN(fileSizeBytes)
            )
            .accounts({
              serviceRequestSubmissionAccount:
                serviceRequestSubmissionAccountPDA,
              bridgeContractState: bridgeContractState.publicKey,
              tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
              aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
              user: endUserKeypair.publicKey,
              systemProgram: SystemProgram.programId,
            })
            .preInstructions([modifyComputeBudgetIx])
            .signers([endUserKeypair])
            .rpc({ skipPreflight: true });

          await confirmTransaction(submitTx);
          console.log(`Service request ${i + 1} submitted successfully`);

          // Verify submission
          const serviceRequestSubmissionData =
            await program.account.serviceRequestSubmissionAccount.fetch(
              serviceRequestSubmissionAccountPDA
            );

          const actualServiceRequestId =
            serviceRequestSubmissionData.serviceRequest.serviceRequestId;

          assert.equal(
            actualServiceRequestId,
            expectedServiceRequestId,
            `Service Request ID should match expected value for request ${
              i + 1
            }`
          );

          serviceRequestIds.push(expectedServiceRequestId);

          // Add delay between submissions
          await sleep(OPERATION_DELAY);
        } catch (error) {
          handleProgramError(error, `Service Request ${i + 1} Submission`);
        }
      }

      // Verify all submissions in temp storage
      const tempServiceRequestsData =
        await program.account.tempServiceRequestsDataAccount.fetch(
          tempServiceRequestsDataAccountPDA
        );

      console.log(
        "Total submitted service requests:",
        tempServiceRequestsData.serviceRequests.length
      );

      serviceRequestIds.forEach((serviceRequestId, index) => {
        const isSubmitted = tempServiceRequestsData.serviceRequests.some(
          (sr) => sr.serviceRequestId === serviceRequestId
        );
        console.log(
          `Service request ${index + 1} status:`,
          `ID: ${serviceRequestId}`,
          `Found: ${isSubmitted ? "Yes" : "No"}`
        );
        assert.isTrue(
          isSubmitted,
          `Service Request ${
            index + 1
          } should be in TempServiceRequestsDataAccount`
        );
      });
    });
  });

  describe("Price Quote Management", () => {
    it("Submits and manages price quotes", async () => {
      const modifyComputeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
        units: COMPUTE_UNITS_PER_TX,
      });

      assert.isTrue(
        serviceRequestIds.length > 0,
        "No service request IDs available"
      );

      const [tempServiceRequestsDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("temp_service_requests_data")],
          program.programId
        );

      const [bridgeNodesDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_nodes_data")],
          program.programId
        );

      for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
        try {
          const serviceRequestId = serviceRequestIds[i];
          if (!serviceRequestId) {
            console.log(`Skipping invalid service request ID at index ${i}`);
            continue;
          }

          const truncatedServiceRequestId = serviceRequestId.substring(0, 24);
          console.log(
            `Processing service request ${i + 1}: ${serviceRequestId}`
          );

          // Get service request data
          const [serviceRequestSubmissionAccountPDA] =
            await PublicKey.findProgramAddressSync(
              [Buffer.from("srq"), Buffer.from(truncatedServiceRequestId)],
              program.programId
            );

          const serviceRequestSubmissionData =
            await program.account.serviceRequestSubmissionAccount.fetch(
              serviceRequestSubmissionAccountPDA
            );

          const fileSizeBytes = new BN(
            serviceRequestSubmissionData.serviceRequest.fileSizeBytes
          ).toNumber();

          const baselinePriceLamports = Math.floor(
            (fileSizeBytes / 1000000) * baselinePriceSol * 1e9
          );

          // Initialize best price quote account
          const [bestPriceQuoteAccountPDA] =
            await PublicKey.findProgramAddressSync(
              [Buffer.from("bpx"), Buffer.from(truncatedServiceRequestId)],
              program.programId
            );

          const initQuoteTx = await program.methods
            .initializeBestPriceQuote(truncatedServiceRequestId)
            .accounts({
              bestPriceQuoteAccount: bestPriceQuoteAccountPDA,
              user: provider.wallet.publicKey,
              systemProgram: SystemProgram.programId,
            })
            .preInstructions([modifyComputeBudgetIx])
            .rpc({ skipPreflight: true });

          await confirmTransaction(initQuoteTx);

          // Submit quotes from each bridge node
          for (const bridgeNode of bridgeNodes) {
            if (!bridgeNode?.keypair?.publicKey) {
              console.error("Invalid bridge node configuration");
              continue;
            }

            const quotedPriceLamports = generateRandomPriceQuote(
              baselinePriceLamports
            );
            console.log(
              "Quote details:",
              `Bridge Node: ${bridgeNode.keypair.publicKey.toBase58()}`,
              `Price: ${quotedPriceLamports.toString()}`
            );

            const [priceQuoteSubmissionAccountPDA] =
              await PublicKey.findProgramAddressSync(
                [
                  Buffer.from("px_quote"),
                  Buffer.from(truncatedServiceRequestId),
                ],
                program.programId
              );

            try {
              const submitQuoteTx = await program.methods
                .submitPriceQuote(
                  bridgeNode.pastelId,
                  serviceRequestId,
                  quotedPriceLamports
                )
                .accounts({
                  priceQuoteSubmissionAccount: priceQuoteSubmissionAccountPDA,
                  bridgeContractState: bridgeContractState.publicKey,
                  tempServiceRequestsDataAccount:
                    tempServiceRequestsDataAccountPDA,
                  user: bridgeNode.keypair.publicKey,
                  bridgeNodesDataAccount: bridgeNodesDataAccountPDA,
                  bestPriceQuoteAccount: bestPriceQuoteAccountPDA,
                  systemProgram: SystemProgram.programId,
                })
                .preInstructions([modifyComputeBudgetIx])
                .signers([bridgeNode.keypair])
                .rpc({ skipPreflight: true });

              await confirmTransaction(submitQuoteTx);
              console.log(
                `Price quote submitted successfully for service request ${
                  i + 1
                }`
              );

              await sleep(OPERATION_DELAY);
            } catch (error) {
              handleProgramError(
                error,
                `Price Quote Submission for Service Request ${i + 1}`
              );
            }
          }

          // Verify best price quote selection
          const bestPriceQuoteData =
            await program.account.bestPriceQuoteReceivedForServiceRequest.fetch(
              bestPriceQuoteAccountPDA
            );

          console.log(
            `Best price quote data for request ${i + 1}:`,
            bestPriceQuoteData
          );

          assert.equal(
            bestPriceQuoteData.serviceRequestId,
            serviceRequestIds[i],
            `Best price quote should be selected for service request ${i + 1}`
          );

          // Add delay between service requests
          await sleep(OPERATION_DELAY);
        } catch (error) {
          handleProgramError(
            error,
            `Service Request ${i + 1} Price Quote Processing`
          );
        }
      }
    });
  });

  describe("Transaction and Account Cleanup", () => {
    it("Verifies final account states", async () => {
      try {
        const [bridgeNodeDataAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("bridge_nodes_data")],
            program.programId
          );

        const [tempServiceRequestsDataAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("temp_service_requests_data")],
            program.programId
          );

        const [serviceRequestTxidMappingDataAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("service_request_txid_map")],
            program.programId
          );

        // Verify bridge nodes data
        const bridgeNodeData =
          await program.account.bridgeNodesDataAccount.fetch(
            bridgeNodeDataAccountPDA
          );

        assert.equal(
          bridgeNodeData.bridgeNodes.length,
          NUM_BRIDGE_NODES,
          "Should have correct number of registered bridge nodes"
        );

        // Verify service requests data
        const tempServiceRequestsData =
          await program.account.tempServiceRequestsDataAccount.fetch(
            tempServiceRequestsDataAccountPDA
          );

        assert.equal(
          tempServiceRequestsData.serviceRequests.length,
          NUMBER_OF_SIMULATED_SERVICE_REQUESTS,
          "Should have correct number of service requests"
        );

        // Check account sizes
        const accountInfos = await Promise.all([
          provider.connection.getAccountInfo(bridgeNodeDataAccountPDA),
          provider.connection.getAccountInfo(tempServiceRequestsDataAccountPDA),
          provider.connection.getAccountInfo(
            serviceRequestTxidMappingDataAccountPDA
          ),
        ]);

        accountInfos.forEach((accountInfo, index) => {
          if (!accountInfo) {
            throw new Error(`Account at index ${index} not found`);
          }
          assert.isTrue(
            accountInfo.data.length <= MAX_ACCOUNT_SIZE,
            `Account size exceeds maximum allowed size of ${MAX_ACCOUNT_SIZE} bytes`
          );
        });

        console.log("Final account states verified successfully");
      } catch (error) {
        handleProgramError(error, "Final Account State Verification");
      }
    });

    it("Verifies compute units usage is within limits", async () => {
      console.log("Total compute units used:", totalComputeUnitsUsed);
      assert.isTrue(
        totalComputeUnitsUsed > 0,
        "Should have tracked compute units usage"
      );
      assert.isTrue(
        totalComputeUnitsUsed <=
          COMPUTE_UNITS_PER_TX *
            (NUM_BRIDGE_NODES + NUMBER_OF_SIMULATED_SERVICE_REQUESTS) *
            3,
        "Total compute units should be within reasonable limits"
      );
    });

    it("Verifies account storage usage is within limits", async () => {
      console.log("Max account storage used:", maxAccountStorageUsed);
      assert.isTrue(
        maxAccountStorageUsed > 0,
        "Should have tracked account storage usage"
      );
      assert.isTrue(
        maxAccountStorageUsed <= MAX_ACCOUNT_SIZE,
        "Account storage should be within size limits"
      );
    });
  });

  // Helper function to assert on numerical values with a margin of error
  const assertApproximatelyEqual = (
    actual: number,
    expected: number,
    tolerance: number = 0.01
  ) => {
    const diff = Math.abs(actual - expected);
    const margin = expected * tolerance;
    assert.isTrue(
      diff <= margin,
      `Expected ${actual} to be approximately equal to ${expected} within ${
        tolerance * 100
      }% tolerance`
    );
  };

  // Helper function to verify account data consistency
  const verifyAccountConsistency = async (
    accountPDA: web3.PublicKey,
    expectedDataSize: number
  ) => {
    const accountInfo = await provider.connection.getAccountInfo(accountPDA);
    if (!accountInfo) {
      throw new Error(`Account at ${accountPDA.toString()} not found`);
    }
    assert.isTrue(
      accountInfo.data.length >= expectedDataSize,
      `Account data size ${accountInfo.data.length} is less than expected ${expectedDataSize}`
    );
    return accountInfo;
  };

  after(async () => {
    try {
      console.log("\nTest Suite Completion Statistics:");
      console.log("--------------------------------");
      console.log("Total compute units used:", totalComputeUnitsUsed);
      console.log("Max account storage used:", maxAccountStorageUsed);
      console.log("Total bridge nodes registered:", bridgeNodes.length);
      console.log(
        "Total service requests processed:",
        serviceRequestIds.length
      );

      // Calculate and log average compute units per transaction
      const totalTransactions =
        bridgeNodes.length + serviceRequestIds.length * 2; // Registration + service request + price quotes
      const avgComputeUnits = Math.floor(
        totalComputeUnitsUsed / totalTransactions
      );
      console.log("Average compute units per transaction:", avgComputeUnits);

      // Log account sizes
      const [bridgeNodeDataAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_nodes_data")],
        program.programId
      );
      const accountInfo = await provider.connection.getAccountInfo(
        bridgeNodeDataAccountPDA
      );
      if (accountInfo) {
        console.log(
          "Final bridge node data account size:",
          accountInfo.data.length
        );
      }

      // Clear arrays
      bridgeNodes = [];
      serviceRequestIds = [];
    } catch (error) {
      console.error("Error in cleanup:", error);
    } finally {
      // Reset counters
      totalComputeUnitsUsed = 0;
      maxAccountStorageUsed = 0;
    }
  });

  // Error event handler for uncaught promise rejections
  process.on("unhandledRejection", (error: Error) => {
    console.error("Unhandled promise rejection:", error);
    console.error("Stack trace:", error.stack);
    process.exit(1);
  });
});
