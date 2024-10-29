import { assert, expect } from "chai";
import Decimal from "decimal.js";
import * as crypto from "crypto";
import * as anchor from "@coral-xyz/anchor";
import { Program, web3, AnchorProvider, BN } from "@coral-xyz/anchor";
import { ComputeBudgetProgram, SystemProgram } from "@solana/web3.js";
import { SolanaPastelBridgeProgram } from "../target/types/solana_pastel_bridge_program";
import IDL from "../target/idl/solana_pastel_bridge_program.json";

const { PublicKey, Keypair, Transaction } = anchor.web3;

// Global state tracking
let bridgeContractState: web3.Keypair;
let bridgeNodes: any[] = [];
let serviceRequestIds: string[] = [];
let totalComputeUnitsUsed = 0;
let maxAccountStorageUsed = 0;

const TURN_ON_STORAGE_AND_COMPUTE_PROFILING = true;
process.env.ANCHOR_PROVIDER_URL = "http://127.0.0.1:8899";
process.env.RUST_LOG =
  "solana_runtime::system_instruction_processor=trace,solana_runtime::message_processor=trace,solana_bpf_loader=debug,solana_rbpf=debug";

// Provider setup
const provider = AnchorProvider.env();
anchor.setProvider(provider);
const program = new Program<SolanaPastelBridgeProgram>(IDL as any, provider);
const admin = provider.wallet;

const programID = new anchor.web3.PublicKey(
  "Ew8ohkPJ3JnWoZ3MWvkn86wYMRJkS385Bsis9TwQJo79"
);
const adminPublicKey = admin.publicKey;

// Constants
const INITIAL_DATA_SIZE = 1024; // Start smaller
const MAX_ACCOUNT_SIZE = 10240; // Limited by Solana
const REALLOC_SIZE = 1024; // Smaller increments
const maxSize = 100 * 1024;
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

// Helper function to generate random price quote
function generateRandomPriceQuote(baselinePriceLamports: number): BN {
  const randomIncrement =
    Math.floor(Math.random() * (baselinePriceLamports / 10)) -
    baselinePriceLamports / 20;
  return new BN(baselinePriceLamports + randomIncrement);
}

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

beforeEach(async () => {
  // Reset state between tests
  await new Promise((resolve) => setTimeout(resolve, 1000));
});

afterEach(async () => {
  // Cleanup any leftover state
  serviceRequestIds = [];
});

// Compute unit tracking
const measureComputeUnitsAndStorage = async (txSignature: string) => {
  if (!TURN_ON_STORAGE_AND_COMPUTE_PROFILING) return;

  for (let attempts = 0; attempts < 5; attempts++) {
    const txDetails = await provider.connection.getParsedTransaction(
      txSignature,
      {
        commitment: "confirmed",
      }
    );

    if (txDetails) {
      if (txDetails.meta?.computeUnitsConsumed) {
        totalComputeUnitsUsed += txDetails.meta.computeUnitsConsumed;
      }

      const accounts = txDetails.transaction.message.accountKeys.map(
        (key) => new web3.PublicKey(key.pubkey)
      );

      for (const account of accounts) {
        const accountInfo = await provider.connection.getAccountInfo(account);
        if (accountInfo && accountInfo.data.length > maxAccountStorageUsed) {
          maxAccountStorageUsed = accountInfo.data.length;
        }
      }
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  console.error(
    `Failed to fetch transaction details for signature: ${txSignature}`
  );
};

const handleError = async (error: any, context: string) => {
  console.error(`Error in ${context}:`, error);

  if (error instanceof anchor.AnchorError) {
    console.error(`Error Code: ${error.error.errorCode.code}`);
    console.error(`Error Message: ${error.error.errorMessage}`);
    if (error.error.origin) {
      console.error(`Error Origin: ${error.error.origin}`);
    }
  }

  // Get transaction logs if available
  if (error.logs) {
    console.error("Transaction logs:", error.logs);
  }

  throw error;
};

// Error parsing helper
const parseError = (error: any) => {
  if (error instanceof anchor.AnchorError) {
    const anchorError = error as anchor.AnchorError;
    console.error(`Error Code: ${anchorError.error.errorCode.code}`);
    console.error(`Error Message: ${anchorError.error.errorMessage}`);
    return anchorError;
  }
  console.error("Unknown error:", error);
  throw error;
};

// Setup and cleanup
before(async () => {
  bridgeContractState = web3.Keypair.generate();
  console.log("Program ID:", programID.toString());
  console.log("Admin ID:", adminPublicKey.toString());
});

after(async () => {
  console.log("Total compute units used:", totalComputeUnitsUsed);
  console.log("Max account storage used:", maxAccountStorageUsed);
});

describe("Solana Pastel Bridge Tests", () => {
  describe("Initialization", () => {
    it("Initializes and expands the bridge contract state", async () => {
      try {
        console.log("Starting PDA generation...");

        // Generate PDAs
        const [rewardPoolAccountPDA] =
          await web3.PublicKey.findProgramAddressSync(
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

        const [serviceRequestTxidMappingDataAccountPDA] =
          await web3.PublicKey.findProgramAddressSync(
            [Buffer.from("service_request_txid_map")],
            program.programId
          );

        const [aggregatedConsensusDataAccountPDA] =
          await web3.PublicKey.findProgramAddressSync(
            [Buffer.from("aggregated_consensus_data")],
            program.programId
          );

        const [tempServiceRequestsDataAccountPDA] =
          await web3.PublicKey.findProgramAddressSync(
            [Buffer.from("temp_service_requests_data")],
            program.programId
          );

        const [regFeeReceivingAccountPDA] =
          await web3.PublicKey.findProgramAddressSync(
            [Buffer.from("reg_fee_receiving_account")],
            program.programId
          );

        // Log PDA addresses
        console.log("RewardPoolAccount PDA:", rewardPoolAccountPDA.toBase58());
        console.log(
          "FeeReceivingContractAccount PDA:",
          feeReceivingContractAccountPDA.toBase58()
        );
        console.log(
          "BridgeNodeDataAccount PDA:",
          bridgeNodeDataAccountPDA.toBase58()
        );
        console.log(
          "ServiceRequestTxidMappingDataAccount PDA:",
          serviceRequestTxidMappingDataAccountPDA.toBase58()
        );
        console.log(
          "AggregatedConsensusDataAccount PDA:",
          aggregatedConsensusDataAccountPDA.toBase58()
        );
        console.log(
          "TempServiceRequestsDataAccount PDA:",
          tempServiceRequestsDataAccountPDA.toBase58()
        );
        console.log(
          "RegFeeReceivingAccount PDA:",
          regFeeReceivingAccountPDA.toBase58()
        );

        // Calculate and fund rent exemption
        const minBalanceForRentExemption =
          await provider.connection.getMinimumBalanceForRentExemption(maxSize);
        console.log(
          "Minimum Balance for Rent Exemption:",
          minBalanceForRentExemption
        );

        const fundTx = new anchor.web3.Transaction().add(
          anchor.web3.SystemProgram.transfer({
            fromPubkey: adminPublicKey,
            toPubkey: bridgeContractState.publicKey,
            lamports: minBalanceForRentExemption,
          })
        );
        const fundTxSignature = await provider.sendAndConfirm(fundTx, []);
        await measureComputeUnitsAndStorage(fundTxSignature);

        // Initialize base contract state
        console.log("Starting base initialization...");
        const initBaseTxSignature = await program.methods
          .initializeBase(adminPublicKey)
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: adminPublicKey,
            systemProgram: web3.SystemProgram.programId,
          })
          .signers([bridgeContractState])
          .rpc();
        await measureComputeUnitsAndStorage(initBaseTxSignature);

        // Initialize core PDAs
        console.log("Starting core PDA initialization...");
        const initCorePDAsTxSignature = await program.methods
          .initializeCorePdas()
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: adminPublicKey,
            bridgeRewardPoolAccount: rewardPoolAccountPDA,
            bridgeEscrowAccount: feeReceivingContractAccountPDA,
            regFeeReceivingAccount: regFeeReceivingAccountPDA,
            systemProgram: web3.SystemProgram.programId,
          })
          .rpc();
        await measureComputeUnitsAndStorage(initCorePDAsTxSignature);

        // Initialize data PDAs
        console.log("Starting data PDA initialization...");
        const initDataPDAsTxSignature = await program.methods
          .initializeDataPdas()
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: adminPublicKey,
            bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
            tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
            serviceRequestTxidMappingDataAccount:
              serviceRequestTxidMappingDataAccountPDA,
            aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
            systemProgram: web3.SystemProgram.programId,
          })
          .preInstructions([
            // Add compute budget instruction
            ComputeBudgetProgram.setComputeUnitLimit({
              units: 400_000, // Increase compute budget
            }),
          ])
          .rpc();
        await measureComputeUnitsAndStorage(initDataPDAsTxSignature);

        // Verify initialization
        let state = await program.account.bridgeContractState.fetch(
          bridgeContractState.publicKey
        );

        assert.ok(
          state.isInitialized,
          "Bridge Contract State should be initialized after first init"
        );
        assert.equal(
          state.adminPubkey.toString(),
          adminPublicKey.toString(),
          "Admin public key should match after first init"
        );

        // Handle reallocation
        let currentSize = 10_240;
        while (currentSize < maxSize) {
          console.log(
            `Expanding Bridge Contract State size from ${currentSize} to ${
              currentSize + 10_240
            }`
          );

          const reallocTxSignature = await program.methods
            .reallocateBridgeState()
            .accounts({
              bridgeContractState: bridgeContractState.publicKey,
              adminPubkey: adminPublicKey,
              tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
              bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
              serviceRequestTxidMappingDataAccount:
                serviceRequestTxidMappingDataAccountPDA,
              aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
              systemProgram: web3.SystemProgram.programId,
            })
            .rpc();

          await measureComputeUnitsAndStorage(reallocTxSignature);
          currentSize += 10_240;
          state = await program.account.bridgeContractState.fetch(
            bridgeContractState.publicKey
          );
          console.log(
            `Bridge Contract State size after expansion: ${currentSize}`
          );
        }

        assert.equal(
          currentSize,
          maxSize,
          "Bridge Contract State should reach the maximum size"
        );
        console.log(
          "Bridge Contract State expanded to the maximum size successfully"
        );
      } catch (error) {
        console.error("Error encountered:", error);
        throw error;
      }
    });
  });

  describe("Reinitialization Prevention", () => {
    it("Prevents reinitialization of BridgeContractState and PDAs", async () => {
      const [rewardPoolAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_reward_pool_account")],
          program.programId
        );
      const [bridgeNodeDataAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_nodes_data")],
          program.programId
        );
      const [tempServiceRequestsDataAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("temp_service_requests_data")],
          program.programId
        );
      const [aggregatedConsensusDataAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
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
            systemProgram: web3.SystemProgram.programId,
          })
          .signers([]) // Remove bridgeContractState from signers
          .rpc();
        assert.fail("Reinitialization should have failed");
      } catch (error) {
        const expectedError = "ContractStateAlreadyInitialized";
        assert.include(error.toString(), expectedError);
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
            systemProgram: web3.SystemProgram.programId,
          })
          .rpc();
        assert.fail("PDA reinitialization should have failed");
      } catch (error) {
        const anchorError = parseError(error);
        assert.equal(
          anchorError.error.errorCode.code,
          "ContractStateAlreadyInitialized",
          "Should throw ContractStateAlreadyInitialized error"
        );
      }

      // Verify state remains unchanged
      const state = await program.account.bridgeContractState.fetch(
        bridgeContractState.publicKey
      );
      assert.ok(
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
      // Verify account initialization first
      const [bridgeNodeDataAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_nodes_data")],
        program.programId
      );

      // Wait for previous initialization to complete
      await new Promise((resolve) => setTimeout(resolve, 1000));

      // Verify account is initialized
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
          console.log(`Funding bridge node ${i + 1} with registration fee...`);
          const fundTx = new Transaction().add(
            SystemProgram.transfer({
              fromPubkey: adminPublicKey,
              toPubkey: bridgeNode.publicKey,
              lamports:
                REGISTRATION_ENTRANCE_FEE_SOL * web3.LAMPORTS_PER_SOL +
                10000000,
            })
          );
          const fundTxSignature = await provider.sendAndConfirm(fundTx, []);
          await measureComputeUnitsAndStorage(fundTxSignature);

          const uniquePastelId = crypto.randomBytes(32).toString("hex");
          const uniquePslAddress = "P" + crypto.randomBytes(33).toString("hex");

          // Transfer registration fee
          console.log(
            `Transferring registration fee from bridge node ${
              i + 1
            } to fee receiving contract account...`
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
          await measureComputeUnitsAndStorage(transferTxSignature);

          // Register bridge node
          console.log(`Registering bridge node ${i + 1}...`);
          const registerTxSignature = await program.methods
            .registerNewBridgeNode(uniquePastelId, uniquePslAddress)
            .accounts({
              bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
              user: bridgeNode.publicKey,
              bridgeRewardPoolAccount: bridgeRewardPoolAccountPDA,
              regFeeReceivingAccount: regFeeReceivingAccountPDA,
              systemProgram: SystemProgram.programId,
            })
            .signers([bridgeNode])
            .rpc();

          await measureComputeUnitsAndStorage(registerTxSignature);

          console.log(
            `Bridge Node ${i + 1} registered successfully:`,
            `Address: ${bridgeNode.publicKey.toBase58()}`,
            `Pastel ID: ${uniquePastelId}`,
            `PSL Address: ${uniquePslAddress}`
          );

          bridgeNodes.push({
            keypair: bridgeNode,
            pastelId: uniquePastelId,
            pslAddress: uniquePslAddress,
          });
        } catch (error) {
          console.error(`Error registering bridge node ${i + 1}:`, error);
          throw error;
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
      const [tempServiceRequestsDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("temp_service_requests_data")],
          program.programId
        );

      // Verify account initialization
      const accountInfo = await provider.connection.getAccountInfo(
        tempServiceRequestsDataAccountPDA
      );
      if (!accountInfo) {
        throw new Error("Temp service requests account not initialized");
      }

      // Add null checks for serviceRequestIds
      if (!serviceRequestIds.length) {
        throw new Error("No service request IDs available");
      }
      console.log(
        "TempServiceRequestsDataAccount PDA:",
        tempServiceRequestsDataAccountPDA.toString()
      );

      const [aggregatedConsensusDataAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("aggregated_consensus_data")],
          program.programId
        );
      console.log(
        "AggregatedConsensusDataAccount PDA:",
        aggregatedConsensusDataAccountPDA.toString()
      );

      const lamports =
        web3.LAMPORTS_PER_SOL *
        COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING;
      const ADDITIONAL_SOL_FOR_ACTUAL_REQUEST = 1;
      const totalFundingLamports =
        lamports + web3.LAMPORTS_PER_SOL * ADDITIONAL_SOL_FOR_ACTUAL_REQUEST;

      for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
        console.log(
          `Generating service request ${
            i + 1
          } of ${NUMBER_OF_SIMULATED_SERVICE_REQUESTS}`
        );

        // Generate and fund end user account
        const endUserKeypair = web3.Keypair.generate();
        console.log(`End user address: ${endUserKeypair.publicKey.toString()}`);

        const transferTransaction = new web3.Transaction().add(
          web3.SystemProgram.transfer({
            fromPubkey: adminPublicKey,
            toPubkey: endUserKeypair.publicKey,
            lamports: totalFundingLamports,
          })
        );
        const transferTxSignature = await provider.sendAndConfirm(
          transferTransaction,
          []
        );
        await measureComputeUnitsAndStorage(transferTxSignature);

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
        const expectedServiceRequestId = expectedServiceRequestIdHash.substring(
          0,
          24
        );

        // Derive submission account PDA
        const [serviceRequestSubmissionAccountPDA] =
          await web3.PublicKey.findProgramAddressSync(
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
        const submitTxSignature = await program.methods
          .submitServiceRequest(
            pastelTicketTypeString,
            fileHash,
            ipfsCid,
            new BN(fileSizeBytes)
          )
          .accounts({
            serviceRequestSubmissionAccount: serviceRequestSubmissionAccountPDA,
            bridgeContractState: bridgeContractState.publicKey,
            tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
            aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
            user: endUserKeypair.publicKey,
            systemProgram: web3.SystemProgram.programId,
          })
          .signers([endUserKeypair])
          .rpc();

        await measureComputeUnitsAndStorage(submitTxSignature);
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
          `Service Request ID should match expected value for request ${i + 1}`
        );

        serviceRequestIds.push(expectedServiceRequestId);
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
      assert.ok(
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
        const serviceRequestId = serviceRequestIds[i];
        if (!serviceRequestId) {
          console.log(`Skipping invalid service request ID at index ${i}`);
          continue;
        }
        const truncatedServiceRequestId = serviceRequestId.substring(0, 24);

        console.log(`Processing service request ${i + 1}: ${serviceRequestId}`);

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

        const initQuoteTxSignature = await program.methods
          .initializeBestPriceQuote(truncatedServiceRequestId)
          .accounts({
            bestPriceQuoteAccount: bestPriceQuoteAccountPDA,
            user: provider.wallet.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .rpc();

        await measureComputeUnitsAndStorage(initQuoteTxSignature);

        // Submit quotes from each bridge node
        for (let j = 0; j < bridgeNodes.length; j++) {
          const bridgeNode = bridgeNodes[j];
          if (!bridgeNode?.keypair?.publicKey) {
            console.error(`Invalid bridge node at index ${j}`);
            continue;
          }

          const quotedPriceLamports = generateRandomPriceQuote(
            baselinePriceLamports
          );
          console.log(
            `Quote from bridge node ${j + 1}:`,
            `Address: ${bridgeNode.keypair.publicKey.toBase58()}`,
            `Price: ${quotedPriceLamports.toString()}`
          );

          const [priceQuoteSubmissionAccountPDA] =
            await PublicKey.findProgramAddressSync(
              [Buffer.from("px_quote"), Buffer.from(truncatedServiceRequestId)],
              program.programId
            );

          try {
            const submitQuoteTxSignature = await program.methods
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
              .signers([bridgeNode.keypair])
              .rpc();

            await measureComputeUnitsAndStorage(submitQuoteTxSignature);
            console.log(
              `Price quote submitted successfully for service request ${i + 1}`
            );
          } catch (error) {
            console.error(
              `Error submitting price quote for service request ${i + 1}:`,
              error
            );
            if (error instanceof anchor.AnchorError) {
              console.error(`Error code: ${error.error.errorCode.number}`);
              console.error(`Error message: ${error.error.errorMessage}`);
            }
            throw error;
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

        const bestQuote =
          bestPriceQuoteData.serviceRequestId === serviceRequestIds[i];
        assert.isTrue(
          bestQuote,
          `Best price quote should be selected for service request ${i + 1}`
        );
      }
    });
  });
});
