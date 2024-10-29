import { assert, expect } from "chai";
import Decimal from "decimal.js";
import * as crypto from "crypto";
import * as anchor from "@coral-xyz/anchor";
import { Program, web3, AnchorProvider, BN } from "@coral-xyz/anchor";
import { ComputeBudgetProgram, SystemProgram } from "@solana/web3.js";
import { SolanaPastelBridgeProgram } from "../target/types/solana_pastel_bridge_program";
import IDL from "../target/idl/solana_pastel_bridge_program.json";

const { PublicKey, Keypair, Transaction } = anchor.web3;

// Provider setup with explicit error handling
process.env.ANCHOR_PROVIDER_URL = "http://127.0.0.1:8899";
process.env.RUST_LOG =
  "solana_runtime::system_instruction_processor=trace,solana_runtime::message_processor=trace,solana_bpf_loader=debug,solana_rbpf=debug";

const provider = AnchorProvider.env();
anchor.setProvider(provider);

const program = new Program<SolanaPastelBridgeProgram>(IDL as any, provider);
const admin = provider.wallet;
const adminPublicKey = admin.publicKey;

// Global state tracking with proper typing
let bridgeContractState: web3.Keypair;
interface BridgeNode {
  keypair: web3.Keypair;
  pastelId: string;
  pslAddress: string;
}
let bridgeNodes: BridgeNode[] = [];
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

// Account size calculations with updated space requirements
const ACCOUNT_SIZES = {
  BRIDGE_NODES:
    8 +
    4 +
    10 *
      (64 + // pastel_id
        32 + // reward_address
        64 + // bridge_node_psl_address
        64 + // registration_entrance_fee_transaction_signature
        8 + // compliance_score
        8 + // reliability_score
        8 + // last_active_timestamp
        4 + // total_price_quotes_submitted
        4 + // total_service_requests_attempted
        4 + // successful_service_requests_count
        4 + // current_streak
        4 + // failed_service_requests_count
        8 + // ban_expiry
        1 + // is_eligible_for_rewards
        1 + // is_recently_active
        1 + // is_reliable
        1), // is_banned
  SERVICE_REQUESTS:
    8 +
    4 +
    10 *
      (32 + // service_request_id
        1 + // service_type
        8 + // first_6_characters_of_hash
        64 + // ipfs_cid
        8 + // file_size_bytes
        32 + // user_sol_address
        1 + // status
        1 + // payment_in_escrow
        8 + // request_expiry
        9 + // Option<u64> timestamps (x7)
        65 + // Option<String> selected_bridge_node_pastelid
        9 + // Option<u64> best_quoted_price
        9 + // Option<u64> escrow_amount
        9 + // Option<u64> service_fee
        65), // Option<String> pastel_txid
  CONSENSUS_DATA:
    8 +
    4 +
    10 *
      (64 + // txid
        16 + // status_weights
        4 + // Vec length for hash_weights
        640 + // hash_weights (10 * (64 + 4))
        8 + // first_6_characters_hash
        8), // last_updated
  TXID_MAPPINGS:
    8 +
    4 +
    10 *
      (32 + // service_request_id
        64), // pastel_txid
  BASE_STATE: 8 + 1 + 1 + 32 * 11, // Discriminator + initialized + paused + 11 pubkeys
};

// Business logic constants
const NUM_BRIDGE_NODES = 3;
const NUMBER_OF_SIMULATED_SERVICE_REQUESTS = 5;
const REGISTRATION_ENTRANCE_FEE_SOL = 0.1;
const COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING = 0.0001;
const MIN_NUMBER_OF_ORACLES = 8;
const MIN_REPORTS_FOR_REWARD = 10;
const BAD_BRIDGE_NODE_INDEX = 5;
const MIN_COMPLIANCE_SCORE_FOR_REWARD = 65_000_000_000; // Updated to fixed-point
const MIN_RELIABILITY_SCORE_FOR_REWARD = 80_000_000_000; // Updated to fixed-point
const BASE_REWARD_AMOUNT_IN_LAMPORTS = 100000;
const baselinePriceUSD = 3;
const solToUsdRate = 130;
const baselinePriceSol = baselinePriceUSD / solToUsdRate;

// Enums matching Rust implementation
const TxidStatusEnum = {
  Invalid: 0,
  PendingMining: 1,
  MinedPendingActivation: 2,
  MinedActivated: 3,
} as const;

const PastelTicketTypeEnum = {
  Sense: 0,
  Cascade: 1,
  Nft: 2,
  InferenceApi: 3,
} as const;

// Helper functions with improved error handling
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const handleProgramError = (error: any, context: string) => {
  console.error(`Error in ${context}:`, error);

  if (error instanceof anchor.AnchorError) {
    console.error(`Error Code: ${error.error.errorCode.code}`);
    console.error(`Error Message: ${error.error.errorMessage}`);
    if (error.error.origin) {
      console.error(`Error Origin: ${error.error.origin}`);
    }
    if (error.program) {
      console.error(`Program ID:`, error.program.toString());
    }
  }

  if (error.logs) {
    console.error("Program logs:", error.logs);
  }

  throw error;
};

async function generateServiceRequestId(
  pastelTicketTypeString: string,
  fileHash: string,
  userPubkey: PublicKey
): Promise<string> {
  const concatenatedStr = `${pastelTicketTypeString}${fileHash}${userPubkey.toString()}`;
  const hash = crypto
    .createHash("sha256")
    .update(concatenatedStr)
    .digest("hex");
  return hash;
}

const logAccountState = async (
  connection: web3.Connection,
  pubkey: web3.PublicKey,
  name: string
) => {
  const info = await connection.getAccountInfo(pubkey);
  console.log(`${name} Account:`, {
    exists: info !== null,
    size: info?.data.length ?? 0,
    lamports: info?.lamports ?? 0,
    owner: info?.owner?.toBase58() ?? "none",
  });
};

const confirmTransaction = async (
  signature: string,
  commitment: web3.Commitment = "confirmed",
  timeout = TX_CONFIRMATION_TIMEOUT
) => {
  const startTime = Date.now();

  try {
    const latestBlockhash = await provider.connection.getLatestBlockhash();
    await provider.connection.confirmTransaction(
      {
        signature,
        blockhash: latestBlockhash.blockhash,
        lastValidBlockHeight: latestBlockhash.lastValidBlockHeight,
      },
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
): number => {
  return ACCOUNT_DISCRIMINATOR_SIZE + 4 + (baseSize + itemSize * maxItems);
};

// Account initialization helper
const initializeAccount = async (
  accountSize: number,
  connection: web3.Connection,
  payer: web3.PublicKey,
  owner: web3.PublicKey
): Promise<web3.Keypair> => {
  const account = web3.Keypair.generate();
  const lamports = await connection.getMinimumBalanceForRentExemption(
    accountSize
  );

  const transaction = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer,
      newAccountPubkey: account.publicKey,
      lamports,
      space: accountSize,
      programId: owner,
    })
  );

  await provider.sendAndConfirm(transaction, [account]);
  return account;
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
          units: COMPUTE_UNITS_PER_TX,
        });

        const modifyComputePriceIx = ComputeBudgetProgram.setComputeUnitPrice({
          microLamports: 50,
        });

        console.log("Starting PDA generation...");

        // Generate PDAs with proper seeds and store bumps
        const [rewardPoolAccountPDA, rewardPoolBump] =
          await PublicKey.findProgramAddress(
            [Buffer.from("bridge_reward_pool_account")],
            program.programId
          );

        const [bridgeEscrowAccountPDA, escrowBump] =
          await PublicKey.findProgramAddress(
            [Buffer.from("bridge_escrow_account")],
            program.programId
          );

        const [bridgeNodeDataAccountPDA, nodeDataBump] =
          await PublicKey.findProgramAddress(
            [Buffer.from("bridge_nodes_data")],
            program.programId
          );

        const [serviceRequestTxidMappingDataAccountPDA, mappingBump] =
          await PublicKey.findProgramAddress(
            [Buffer.from("service_request_txid_map")],
            program.programId
          );

        const [aggregatedConsensusDataAccountPDA, consensusBump] =
          await PublicKey.findProgramAddress(
            [Buffer.from("aggregated_consensus_data")],
            program.programId
          );

        const [tempServiceRequestsDataAccountPDA, tempRequestsBump] =
          await PublicKey.findProgramAddress(
            [Buffer.from("temp_service_requests_data")],
            program.programId
          );

        const [regFeeReceivingAccountPDA, regFeeBump] =
          await PublicKey.findProgramAddress(
            [Buffer.from("reg_fee_receiving_account")],
            program.programId
          );

        // Log PDA addresses and bumps
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
          bumps: {
            rewardPool: rewardPoolBump,
            escrow: escrowBump,
            nodeData: nodeDataBump,
            mapping: mappingBump,
            consensus: consensusBump,
            tempRequests: tempRequestsBump,
            regFee: regFeeBump,
          },
        });

        // Calculate precise space requirements for bridge contract state
        const connection = provider.connection;
        const baseStateSpace = ACCOUNT_SIZES.BASE_STATE;

        // Get exact rent exemption amount
        const rent = await connection.getMinimumBalanceForRentExemption(
          baseStateSpace
        );

        // Fund the bridge contract state account with exact rent requirement
        const fundTx = new Transaction().add(
          SystemProgram.transfer({
            fromPubkey: provider.wallet.publicKey,
            toPubkey: bridgeContractState.publicKey,
            lamports: rent,
          })
        );

        const fundTxSignature = await provider.sendAndConfirm(fundTx);
        await confirmTransaction(fundTxSignature);
        console.log("Bridge contract state funded with exact rent:", rent);

        // Verify the account was funded
        const accountInfo = await connection.getAccountInfo(
          bridgeContractState.publicKey
        );
        assert(
          accountInfo !== null,
          "Bridge contract state account should exist"
        );
        assert.equal(
          accountInfo.lamports,
          rent,
          "Account should have exact rent amount"
        );

        // Initialize base state
        console.log("Starting base initialization...");
        const initBaseTx = await program.methods
          .initializeBase(provider.wallet.publicKey)
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: provider.wallet.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx, modifyComputePriceIx])
          .signers([bridgeContractState])
          .rpc({
            skipPreflight: true,
            commitment: "confirmed",
          });

        await confirmTransaction(initBaseTx);
        console.log("Base initialization complete");

        // Add delay between transactions
        await sleep(2000);

        // Initialize core PDAs
        console.log("Starting core PDA initialization...");

        // First verify the contract state
        const stateAccount = await program.account.bridgeContractState.fetch(
          bridgeContractState.publicKey
        );
        console.log("Contract state before core init:", stateAccount);

        // Build transaction with required space
        const corePDAInstructionAccounts = {
          bridgeContractState: bridgeContractState.publicKey,
          user: provider.wallet.publicKey,
          bridgeRewardPoolAccount: rewardPoolAccountPDA,
          bridgeEscrowAccount: bridgeEscrowAccountPDA,
          regFeeReceivingAccount: regFeeReceivingAccountPDA,
          systemProgram: SystemProgram.programId,
        };

        // Log all account states before transaction
        await Promise.all([
          logAccountState(
            connection,
            bridgeContractState.publicKey,
            "Bridge Contract State"
          ),
          logAccountState(connection, rewardPoolAccountPDA, "Reward Pool"),
          logAccountState(connection, bridgeEscrowAccountPDA, "Escrow"),
          logAccountState(connection, regFeeReceivingAccountPDA, "Reg Fee"),
        ]);

        // Build the transaction with explicit compute budget
        const corePDATx = new Transaction();
        corePDATx.add(
          ComputeBudgetProgram.setComputeUnitLimit({
            units: COMPUTE_UNITS_PER_TX,
          })
        );

        // Add the core PDA initialization instruction
        const initCorePDAsIx = await program.methods
          .initializeCorePdas()
          .accounts(corePDAInstructionAccounts)
          .instruction();

        corePDATx.add(initCorePDAsIx);

        // Send and confirm with detailed error handling
        try {
          const signature = await provider.sendAndConfirm(corePDATx, [], {
            skipPreflight: true,
            commitment: "confirmed",
            preflightCommitment: "confirmed",
          });

          console.log("Core PDAs initialization signature:", signature);

          // Wait for confirmation with retries
          for (let i = 0; i < 3; i++) {
            try {
              await provider.connection.confirmTransaction(
                {
                  signature,
                  ...(await provider.connection.getLatestBlockhash()),
                },
                "confirmed"
              );
              break;
            } catch (e) {
              if (i === 2) throw e;
              await sleep(1000);
            }
          }

          // Verify accounts after initialization
          await Promise.all([
            logAccountState(
              connection,
              rewardPoolAccountPDA,
              "Reward Pool After"
            ),
            logAccountState(connection, bridgeEscrowAccountPDA, "Escrow After"),
            logAccountState(
              connection,
              regFeeReceivingAccountPDA,
              "Reg Fee After"
            ),
          ]);

          // Verify the state was updated
          const updatedState = await program.account.bridgeContractState.fetch(
            bridgeContractState.publicKey
          );
          console.log("Contract state after core init:", updatedState);

          console.log("Core PDAs initialization complete");
        } catch (error) {
          console.error("Core PDA initialization failed with error:", error);
          if (error.logs) {
            console.error("Program logs:", error.logs);
          }
          // Get detailed error information if available
          try {
            const simulationResult =
              await provider.connection.simulateTransaction(corePDATx);
            console.error("Simulation result:", simulationResult);
          } catch (simError) {
            console.error("Simulation failed:", simError);
          }
          throw error;
        }
        await sleep(2000);

        console.log("Starting sequential data PDA initialization...");
        // Log initial states of all accounts
        await Promise.all([
          logAccountState(
            connection,
            bridgeNodeDataAccountPDA,
            "Bridge Node Data Initial"
          ),
          logAccountState(
            connection,
            tempServiceRequestsDataAccountPDA,
            "Temp Service Requests Initial"
          ),
          logAccountState(
            connection,
            serviceRequestTxidMappingDataAccountPDA,
            "TXID Mapping Initial"
          ),
          logAccountState(
            connection,
            aggregatedConsensusDataAccountPDA,
            "Consensus Data Initial"
          ),
        ]);

        // Initialize Bridge Nodes Data Account
        try {
          console.log("Initializing Bridge Nodes Data Account...");
          const bridgeNodesTx = new Transaction().add(
            ComputeBudgetProgram.setComputeUnitLimit({
              units: COMPUTE_UNITS_PER_TX,
            })
          );

          console.log("About to create bridgeNodesIx...");
          const bridgeNodesIx = await program.methods
            .initializeBridgeNodesData()
            .accounts({
              bridgeContractState: bridgeContractState.publicKey,
              user: provider.wallet.publicKey,
              bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
              systemProgram: SystemProgram.programId,
            })
            .instruction();
          console.log("bridgeNodesIx:", bridgeNodesIx);

          bridgeNodesTx.add(bridgeNodesIx);

          const bridgeNodesSignature = await provider.sendAndConfirm(
            bridgeNodesTx,
            [],
            {
              skipPreflight: true,
              commitment: "confirmed",
            }
          );
          console.log(
            "Bridge Nodes Data initialization signature:",
            bridgeNodesSignature
          );
          await sleep(2000);
        } catch (error) {
          console.error("Bridge Nodes Data initialization failed:", error);
          throw error;
        }

        // Initialize Temp Service Requests Account
        try {
          console.log("Initializing Temp Service Requests Account...");
          const tempRequestsTx = new Transaction().add(
            ComputeBudgetProgram.setComputeUnitLimit({
              units: COMPUTE_UNITS_PER_TX,
            })
          );

          const tempRequestsIx = await program.methods
            .initializeTempRequestsData()
            .accounts({
              bridgeContractState: bridgeContractState.publicKey,
              user: provider.wallet.publicKey,
              tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
              systemProgram: SystemProgram.programId,
            })
            .instruction();

          tempRequestsTx.add(tempRequestsIx);

          const tempRequestsSignature = await provider.sendAndConfirm(
            tempRequestsTx,
            [],
            {
              skipPreflight: true,
              commitment: "confirmed",
            }
          );
          console.log(
            "Temp Service Requests initialization signature:",
            tempRequestsSignature
          );
          await sleep(2000);
        } catch (error) {
          console.error("Temp Service Requests initialization failed:", error);
          throw error;
        }

        // Initialize TXID Mapping Account
        try {
          console.log("Initializing TXID Mapping Account...");
          const txidMappingTx = new Transaction().add(
            ComputeBudgetProgram.setComputeUnitLimit({
              units: COMPUTE_UNITS_PER_TX,
            })
          );

          const txidMappingIx = await program.methods
            .initializeTxidMappingData()
            .accounts({
              bridgeContractState: bridgeContractState.publicKey,
              user: provider.wallet.publicKey,
              serviceRequestTxidMappingDataAccount:
                serviceRequestTxidMappingDataAccountPDA,
              systemProgram: SystemProgram.programId,
            })
            .instruction();

          txidMappingTx.add(txidMappingIx);

          const txidMappingSignature = await provider.sendAndConfirm(
            txidMappingTx,
            [],
            {
              skipPreflight: true,
              commitment: "confirmed",
            }
          );
          console.log(
            "TXID Mapping initialization signature:",
            txidMappingSignature
          );
          await sleep(2000);
        } catch (error) {
          console.error("TXID Mapping initialization failed:", error);
          throw error;
        }

        // Initialize Consensus Data Account
        try {
          console.log("Initializing Consensus Data Account...");
          const consensusDataTx = new Transaction().add(
            ComputeBudgetProgram.setComputeUnitLimit({
              units: COMPUTE_UNITS_PER_TX,
            })
          );

          console.log("About to create consensusDataIx...");
          const consensusDataIx = await program.methods
            .initializeConsensusData()
            .accounts({
              bridgeContractState: bridgeContractState.publicKey,
              user: provider.wallet.publicKey,
              aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
              systemProgram: SystemProgram.programId,
            })
            .instruction();
          console.log("consensusDataIx:", consensusDataIx);

          consensusDataTx.add(consensusDataIx);

          const consensusDataSignature = await provider.sendAndConfirm(
            consensusDataTx,
            [],
            {
              skipPreflight: true,
              commitment: "confirmed",
            }
          );
          console.log(
            "Consensus Data initialization signature:",
            consensusDataSignature
          );
          await sleep(2000);
        } catch (error) {
          console.error("Consensus Data initialization failed:", error);
          throw error;
        }

        // Verify final states of all accounts
        console.log("Verifying final account states...");
        await Promise.all([
          logAccountState(
            connection,
            bridgeNodeDataAccountPDA,
            "Bridge Node Data Final"
          ),
          logAccountState(
            connection,
            tempServiceRequestsDataAccountPDA,
            "Temp Service Requests Final"
          ),
          logAccountState(
            connection,
            serviceRequestTxidMappingDataAccountPDA,
            "TXID Mapping Final"
          ),
          logAccountState(
            connection,
            aggregatedConsensusDataAccountPDA,
            "Consensus Data Final"
          ),
        ]);

        // Verify final bridge contract state
        const finalState = await program.account.bridgeContractState.fetch(
          bridgeContractState.publicKey
        );
        console.log("Final contract state:", finalState);

        // Comprehensive state verification
        const verifyState = () => {
          assert.isTrue(
            finalState.isInitialized,
            "Bridge contract state should be initialized"
          );
          assert.isFalse(
            finalState.isPaused,
            "Bridge contract should not be paused initially"
          );

          const stateChecks = [
            {
              actual: finalState.adminPubkey,
              expected: provider.wallet.publicKey,
              name: "Admin public key",
            },
            {
              actual: finalState.bridgeNodesDataAccountPubkey,
              expected: bridgeNodeDataAccountPDA,
              name: "Bridge nodes data account",
            },
            {
              actual: finalState.tempServiceRequestsDataAccountPubkey,
              expected: tempServiceRequestsDataAccountPDA,
              name: "Temp service requests data account",
            },
            {
              actual: finalState.aggregatedConsensusDataAccountPubkey,
              expected: aggregatedConsensusDataAccountPDA,
              name: "Aggregated consensus data account",
            },
            {
              actual: finalState.serviceRequestTxidMappingAccountPubkey,
              expected: serviceRequestTxidMappingDataAccountPDA,
              name: "Service request txid mapping account",
            },
            {
              actual: finalState.bridgeRewardPoolAccountPubkey,
              expected: rewardPoolAccountPDA,
              name: "Bridge reward pool account",
            },
            {
              actual: finalState.bridgeEscrowAccountPubkey,
              expected: bridgeEscrowAccountPDA,
              name: "Bridge escrow account",
            },
            {
              actual: finalState.regFeeReceivingAccountPubkey,
              expected: regFeeReceivingAccountPDA,
              name: "Registration fee receiving account",
            },
          ];

          stateChecks.forEach((check) => {
            assert.equal(
              check.actual.toString(),
              check.expected.toString(),
              `${check.name} should be set correctly`
            );
          });
        };

        // Run final state verification
        verifyState();
        console.log("Data PDAs initialization and verification complete");

        // Verify rent-exemption status for all accounts
        const verifyRentExemption = async () => {
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
            assert(accountInfo !== null, `${account.name} account not found`);

            const rentExempt =
              await connection.getMinimumBalanceForRentExemption(
                accountInfo.data.length
              );

            assert.isTrue(
              accountInfo.lamports >= rentExempt,
              `${account.name} account should be rent-exempt. Required: ${rentExempt}, Current: ${accountInfo.lamports}`
            );
          }
        };

        await verifyRentExemption();

        // Verify data structure initialization
        const verifyDataStructures = async () => {
          // Verify bridge nodes data
          const bridgeNodesData =
            await program.account.bridgeNodesDataAccount.fetch(
              bridgeNodeDataAccountPDA
            );
          assert.isArray(
            bridgeNodesData.bridgeNodes,
            "Bridge nodes should be initialized as array"
          );
          assert.equal(
            bridgeNodesData.bridgeNodes.length,
            0,
            "Bridge nodes array should be empty"
          );

          // Verify service requests data
          const tempServiceRequestsData =
            await program.account.tempServiceRequestsDataAccount.fetch(
              tempServiceRequestsDataAccountPDA
            );
          assert.isArray(
            tempServiceRequestsData.serviceRequests,
            "Service requests should be initialized as array"
          );
          assert.equal(
            tempServiceRequestsData.serviceRequests.length,
            0,
            "Service requests array should be empty"
          );

          // Verify txid mapping data
          const txidMappingData =
            await program.account.serviceRequestTxidMappingDataAccount.fetch(
              serviceRequestTxidMappingDataAccountPDA
            );
          assert.isArray(
            txidMappingData.mappings,
            "TXID mappings should be initialized as array"
          );
          assert.equal(
            txidMappingData.mappings.length,
            0,
            "TXID mappings array should be empty"
          );

          // Verify consensus data
          const consensusData =
            await program.account.aggregatedConsensusDataAccount.fetch(
              aggregatedConsensusDataAccountPDA
            );
          assert.isArray(
            consensusData.consensusData,
            "Consensus data should be initialized as array"
          );
          assert.equal(
            consensusData.consensusData.length,
            0,
            "Consensus data array should be empty"
          );
        };

        await verifyDataStructures();

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

      // Get PDAs with proper error handling
      let rewardPoolAccountPDA: PublicKey,
        bridgeNodeDataAccountPDA: PublicKey,
        tempServiceRequestsDataAccountPDA: PublicKey,
        aggregatedConsensusDataAccountPDA: PublicKey,
        regFeeReceivingAccountPDA: PublicKey,
        bridgeEscrowAccountPDA: PublicKey;

      try {
        [rewardPoolAccountPDA] = await PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_reward_pool_account")],
          program.programId
        );
        [bridgeNodeDataAccountPDA] = await PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_nodes_data")],
          program.programId
        );
        [tempServiceRequestsDataAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("temp_service_requests_data")],
            program.programId
          );
        [aggregatedConsensusDataAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("aggregated_consensus_data")],
            program.programId
          );
        [regFeeReceivingAccountPDA] = await PublicKey.findProgramAddressSync(
          [Buffer.from("reg_fee_receiving_account")],
          program.programId
        );
        [bridgeEscrowAccountPDA] = await PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_escrow_account")],
          program.programId
        );
      } catch (error) {
        console.error("Error deriving PDAs:", error);
        throw error;
      }

      // Create a new keypair for testing reinitialization
      const newBridgeContractState = web3.Keypair.generate();

      // Fund the new contract state account
      const connection = provider.connection;
      const rent = await connection.getMinimumBalanceForRentExemption(
        ACCOUNT_SIZES.BASE_STATE
      );

      const fundTx = new Transaction().add(
        SystemProgram.transfer({
          fromPubkey: provider.wallet.publicKey,
          toPubkey: newBridgeContractState.publicKey,
          lamports: rent,
        })
      );

      await provider.sendAndConfirm(fundTx);

      // Try to reinitialize base state
      try {
        await program.methods
          .initializeBase(adminPublicKey)
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: provider.wallet.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .signers([provider.wallet.payer]) // Add the admin as signer
          .rpc();
        assert.fail("Should not be able to reinitialize base state");
      } catch (error) {
        // Should get ContractStateAlreadyInitialized error
        if (error instanceof anchor.AnchorError) {
          assert.equal(
            error.error.errorCode.code,
            "ContractStateAlreadyInitialized",
            "Should get ContractStateAlreadyInitialized error"
          );
        } else {
          throw error; // Re-throw unexpected errors
        }
      }

      // Try to reinitialize core PDAs
      try {
        await program.methods
          .initializeCorePdas()
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: provider.wallet.publicKey,
            bridgeRewardPoolAccount: rewardPoolAccountPDA,
            bridgeEscrowAccount: bridgeEscrowAccountPDA,
            regFeeReceivingAccount: regFeeReceivingAccountPDA,
            systemProgram: SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .signers([provider.wallet.payer]) // Add the admin as signer
          .rpc();
        assert.fail("Should not be able to reinitialize core PDAs");
      } catch (error) {
        if (error instanceof anchor.AnchorError) {
          assert.include(
            error.error.errorMessage,
            "The program expected this account to be uninitialized"
          );
        }
      }

      // Try to reinitialize bridge nodes data
      try {
        await program.methods
          .initializeBridgeNodesData()
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: provider.wallet.publicKey,
            bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
            systemProgram: SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .signers([provider.wallet.payer]) // Add the admin as signer
          .rpc();
        assert.fail("Should not be able to reinitialize bridge nodes data");
      } catch (error) {
        if (error instanceof anchor.AnchorError) {
          assert.include(
            error.error.errorMessage,
            "The program expected this account to be uninitialized"
          );
        }
      }

      // Try to reinitialize temp requests data
      try {
        await program.methods
          .initializeTempRequestsData()
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: provider.wallet.publicKey,
            tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
            systemProgram: SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .signers([provider.wallet.payer]) // Add the admin as signer
          .rpc();
        assert.fail("Should not be able to reinitialize temp requests data");
      } catch (error) {
        if (error instanceof anchor.AnchorError) {
          assert.include(
            error.error.errorMessage,
            "The program expected this account to be uninitialized"
          );
        }
      }

      // Try to reinitialize consensus data
      try {
        await program.methods
          .initializeConsensusData()
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            user: provider.wallet.publicKey,
            aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
            systemProgram: SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .signers([provider.wallet.payer]) // Add the admin as signer
          .rpc();
        assert.fail("Should not be able to reinitialize consensus data");
      } catch (error) {
        if (error instanceof anchor.AnchorError) {
          assert.include(
            error.error.errorMessage,
            "The program expected this account to be uninitialized"
          );
        }
      }

      // Add delay to ensure state is settled
      await sleep(OPERATION_DELAY);

      // Verify state remains unchanged
      const state = await program.account.bridgeContractState.fetch(
        bridgeContractState.publicKey
      );

      // Verify all state properties remain unchanged
      assert.isTrue(
        state.isInitialized,
        "Bridge Contract State should still be initialized"
      );
      assert.equal(
        state.adminPubkey.toString(),
        adminPublicKey.toString(),
        "Admin public key should remain unchanged"
      );
      assert.isFalse(state.isPaused, "Bridge contract should not be paused");
      assert.equal(
        state.bridgeNodesDataAccountPubkey.toString(),
        bridgeNodeDataAccountPDA.toString(),
        "Bridge nodes data account pubkey should remain unchanged"
      );
      assert.equal(
        state.bridgeRewardPoolAccountPubkey.toString(),
        rewardPoolAccountPDA.toString(),
        "Reward pool account pubkey should remain unchanged"
      );
      assert.equal(
        state.bridgeEscrowAccountPubkey.toString(),
        bridgeEscrowAccountPDA.toString(),
        "Escrow account pubkey should remain unchanged"
      );

      // Log final state for debugging
      console.log("Final contract state verification successful:", {
        isInitialized: state.isInitialized,
        isPaused: state.isPaused,
        adminPubkey: state.adminPubkey.toString(),
        bridgeNodesDataPubkey: state.bridgeNodesDataAccountPubkey.toString(),
        rewardPoolPubkey: state.bridgeRewardPoolAccountPubkey.toString(),
        escrowPubkey: state.bridgeEscrowAccountPubkey.toString(),
      });
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

  it("Submits service requests", async () => {
    const modifyComputeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
      units: COMPUTE_UNITS_PER_TX,
    });

    // Get PDAs with proper error handling
    let tempServiceRequestsDataAccountPDA: PublicKey;
    let aggregatedConsensusDataAccountPDA: PublicKey;

    try {
      [tempServiceRequestsDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("temp_service_requests_data")],
          program.programId
        );
      [aggregatedConsensusDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("aggregated_consensus_data")],
          program.programId
        );
    } catch (error) {
      console.error("Error deriving PDAs:", error);
      throw error;
    }

    // Verify account initialization with retries
    let accountInfo = null;
    for (let attempt = 0; attempt < 3; attempt++) {
      accountInfo = await provider.connection.getAccountInfo(
        tempServiceRequestsDataAccountPDA
      );
      if (accountInfo) break;
      await sleep(1000);
    }

    if (!accountInfo) {
      throw new Error(
        "Temp service requests account not initialized after retries"
      );
    }

    const lamports =
      web3.LAMPORTS_PER_SOL * COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING;
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
        console.log(`End user address: ${endUserKeypair.publicKey.toString()}`);

        const transferTx = new Transaction().add(
          SystemProgram.transfer({
            fromPubkey: adminPublicKey,
            toPubkey: endUserKeypair.publicKey,
            lamports: totalFundingLamports,
          })
        );

        const transferTxSignature = await provider.sendAndConfirm(transferTx);
        await confirmTransaction(transferTxSignature);

        // Generate request data with error checking
        const fileHash = crypto
          .createHash("sha3-256")
          .update(`file${i}`)
          .digest("hex")
          .substring(0, 6);

        if (fileHash.length !== 6) {
          throw new Error(`Invalid file hash length: ${fileHash.length}`);
        }

        const pastelTicketTypeString =
          Object.keys(PastelTicketTypeEnum)[
            i % Object.keys(PastelTicketTypeEnum).length
          ];

        if (!pastelTicketTypeString) {
          throw new Error("Invalid Pastel ticket type");
        }

        const ipfsCid = `Qm${crypto.randomBytes(44).toString("hex")}`;
        const fileSizeBytes = Math.floor(Math.random() * 1000000) + 1;

        // Generate service request ID using the program's method
        const serviceRequestId = await generateServiceRequestId(
          pastelTicketTypeString,
          fileHash,
          endUserKeypair.publicKey
        );

        // Derive submission account PDA using the truncated service request ID
        const truncatedServiceRequestIdBytes = Buffer.from(
          serviceRequestId
        ).subarray(0, 12);
        const [serviceRequestSubmissionAccountPDA] =
          await PublicKey.findProgramAddressSync(
            [Buffer.from("srq"), truncatedServiceRequestIdBytes],
            program.programId
          );

        console.log({
          fileHash,
          pastelTicketTypeString,
          ipfsCid,
          fileSizeBytes,
          serviceRequestId,
          truncatedServiceRequestId: serviceRequestId.substring(0, 24), // Log truncated ID for verification
          submissionAccountPDA: serviceRequestSubmissionAccountPDA.toString(),
        });

        // Submit service request with retries
        let submitTx;
        let retryCount = 0;
        const maxRetries = 3;

        while (retryCount < maxRetries) {
          try {
            submitTx = await program.methods
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
                tempServiceRequestsDataAccount:
                  tempServiceRequestsDataAccountPDA,
                aggregatedConsensusDataAccount:
                  aggregatedConsensusDataAccountPDA,
                user: endUserKeypair.publicKey,
                systemProgram: SystemProgram.programId,
              })
              .preInstructions([modifyComputeBudgetIx])
              .signers([endUserKeypair])
              .rpc({
                skipPreflight: true,
                commitment: "confirmed",
              });
            break;
          } catch (error) {
            retryCount++;
            if (retryCount === maxRetries) {
              throw error;
            }
            await sleep(1000);
          }
        }

        if (!submitTx) {
          throw new Error("Failed to submit service request after retries");
        }

        await confirmTransaction(submitTx);
        console.log(`Service request ${i + 1} submitted successfully`);

        // Verify submission with retries
        let serviceRequestSubmissionData = null;
        retryCount = 0;

        while (retryCount < maxRetries) {
          try {
            serviceRequestSubmissionData =
              await program.account.serviceRequestSubmissionAccount.fetch(
                serviceRequestSubmissionAccountPDA
              );
            break;
          } catch (error) {
            retryCount++;
            if (retryCount === maxRetries) {
              throw error;
            }
            await sleep(1000);
          }
        }

        if (!serviceRequestSubmissionData) {
          throw new Error(
            "Failed to fetch service request submission data after retries"
          );
        }

        const actualServiceRequestId =
          serviceRequestSubmissionData.serviceRequest.serviceRequestId;

        assert.equal(
          actualServiceRequestId,
          serviceRequestId,
          `Service Request ID should match expected value for request ${i + 1}`
        );

        serviceRequestIds.push(serviceRequestId);

        // Add delay between submissions
        await sleep(OPERATION_DELAY);
      } catch (error) {
        console.error(`Service Request ${i + 1} Submission failed:`, error);
        if (error.logs) {
          console.error("Program logs:", error.logs);
        }
        throw error;
      }
    }

    // Verify all submissions in temp storage with retries
    let tempServiceRequestsData = null;
    let retryCount = 0;
    const maxRetries = 3;

    while (retryCount < maxRetries) {
      try {
        tempServiceRequestsData =
          await program.account.tempServiceRequestsDataAccount.fetch(
            tempServiceRequestsDataAccountPDA
          );
        break;
      } catch (error) {
        retryCount++;
        if (retryCount === maxRetries) {
          throw error;
        }
        await sleep(1000);
      }
    }

    if (!tempServiceRequestsData) {
      throw new Error(
        "Failed to fetch temp service requests data after retries"
      );
    }

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

    // Final verification
    assert.equal(
      tempServiceRequestsData.serviceRequests.length,
      NUMBER_OF_SIMULATED_SERVICE_REQUESTS,
      "All service requests should be submitted successfully"
    );
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
