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
const NUM_BRIDGE_NODES = 12;
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
      bridgeContractState = web3.Keypair.generate();
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
        await sleep(250);

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
              await sleep(250);
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
        await sleep(250);

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
          await sleep(250);
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
          await sleep(250);
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
          await sleep(250);
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
          await sleep(250);
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

  describe("Service Requests", () => {
    before(async () => {
      // Make sure bridge contract state is initialized first
      const state = await program.account.bridgeContractState.fetch(
        bridgeContractState.publicKey
      );
      assert(
        state.isInitialized,
        "Bridge contract state must be initialized first"
      );

      const [tempServiceRequestsDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("temp_service_requests_data")],
          program.programId
        );

      const accountInfo = await provider.connection.getAccountInfo(
        tempServiceRequestsDataAccountPDA
      );

      if (!accountInfo) {
        console.log("Initializing temp service requests account...");
        try {
          const tx = await program.methods
            .initializeTempRequestsData()
            .accounts({
              bridgeContractState: bridgeContractState.publicKey,
              user: provider.wallet.publicKey,
              tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
              systemProgram: SystemProgram.programId,
            })
            .signers([provider.wallet.payer])
            .rpc();

          await confirmTransaction(tx);
          console.log("Temp service requests account initialized");
        } catch (error) {
          console.error(
            "Failed to initialize temp service requests account:",
            error
          );
          throw error;
        }
      }
    });
  });

  describe("Core Bridge Functionality", () => {
    let userKeypair: web3.Keypair;
    let fileHash: string;
    let ipfsCid: string;
    let serviceRequestId: string;
    let bestPriceQuoteAccount: web3.PublicKey;
    let serviceRequestPDA: web3.PublicKey;
    let bestPriceQuoteBump: number;

    before(async () => {
      // Create and fund user account
      userKeypair = web3.Keypair.generate();
      const fundTx = new web3.Transaction().add(
        web3.SystemProgram.transfer({
          fromPubkey: adminPublicKey,
          toPubkey: userKeypair.publicKey,
          lamports: web3.LAMPORTS_PER_SOL * 10,
        })
      );
      await provider.sendAndConfirm(fundTx);
      console.log("User account funded:", userKeypair.publicKey.toString());
    });

    it("Submits a service request", async () => {
      // Generate test data
      fileHash = "abc123"; // First 6 chars of SHA3-256
      ipfsCid = "QmW8rAgr5Levy1qyXqBRm24e4XhiDNBqhcDRnZ7puKytcG";
      const fileSizeBytes = 1024 * 1024; // 1MB

      // Generate service request ID
      serviceRequestId = await generateServiceRequestId(
        "Sense",
        fileHash,
        userKeypair.publicKey
      );
      console.log("Generated service request ID:", serviceRequestId);

      // Find PDAs
      [serviceRequestPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("srq"), Buffer.from(serviceRequestId.slice(0, 12))],
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

      // Submit service request with compute budget instruction
      const modifyComputeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
        units: COMPUTE_UNITS_PER_TX,
      });

      try {
        const tx = await program.methods
          .submitServiceRequest(
            "Sense",
            fileHash,
            ipfsCid,
            new BN(fileSizeBytes)
          )
          .accounts({
            serviceRequestSubmissionAccount: serviceRequestPDA,
            bridgeContractState: bridgeContractState.publicKey,
            user: userKeypair.publicKey,
            tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
            aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
            systemProgram: web3.SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .signers([userKeypair])
          .rpc();

        await confirmTransaction(tx);
        console.log(
          "Service request submitted successfully:",
          serviceRequestId
        );

        // Verify the service request was stored
        const tempRequests =
          await program.account.tempServiceRequestsDataAccount.fetch(
            tempServiceRequestsDataAccountPDA
          );
        const request = tempRequests.serviceRequests.find(
          (r) => r.serviceRequestId === serviceRequestId
        );
        assert(request, "Service request should be stored");
        assert.equal(request.status.pending, true, "Status should be pending");
      } catch (error) {
        handleProgramError(error, "Service Request Submission");
      }
    });

    it("Initializes best price quote tracking", async () => {
      await sleep(250); // Add delay between transactions

      // Generate PDA for best price quote account
      [bestPriceQuoteAccount, bestPriceQuoteBump] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("bpx"), Buffer.from(serviceRequestId.slice(0, 12))],
          program.programId
        );

      console.log("Best price quote PDA:", bestPriceQuoteAccount.toString());

      try {
        const modifyComputeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
          units: COMPUTE_UNITS_PER_TX,
        });

        const tx = await program.methods
          .initializeBestPriceQuote(serviceRequestId)
          .accounts({
            bestPriceQuoteAccount,
            user: provider.wallet.publicKey,
            systemProgram: web3.SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .rpc();

        await confirmTransaction(tx);
        console.log("Best price quote tracking initialized");

        // Verify the account was initialized
        const bestQuoteAccount =
          await program.account.bestPriceQuoteReceivedForServiceRequest.fetch(
            bestPriceQuoteAccount
          );
        assert(
          bestQuoteAccount,
          "Best price quote account should be initialized"
        );
        assert.equal(
          bestQuoteAccount.serviceRequestId,
          serviceRequestId,
          "Service request ID should match"
        );
      } catch (error) {
        handleProgramError(error, "Best Price Quote Initialization");
      }
    });

    it("Bridge nodes submit price quotes", async () => {
      await sleep(250); // Add delay between transactions

      const [bridgeNodeDataAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_nodes_data")],
        program.programId
      );

      const [tempServiceRequestsDataAccountPDA] =
        await PublicKey.findProgramAddressSync(
          [Buffer.from("temp_service_requests_data")],
          program.programId
        );

      // Have each bridge node submit a quote
      for (const bridgeNode of bridgeNodes) {
        const baselinePriceLamports = baselinePriceSol * web3.LAMPORTS_PER_SOL;
        const quotedPrice = generateRandomPriceQuote(baselinePriceLamports);

        // Generate PDA for price quote submission
        const [priceQuotePDA] = await PublicKey.findProgramAddressSync(
          [Buffer.from("px_quote"), Buffer.from(serviceRequestId)],
          program.programId
        );

        try {
          const modifyComputeBudgetIx =
            ComputeBudgetProgram.setComputeUnitLimit({
              units: COMPUTE_UNITS_PER_TX,
            });

          const tx = await program.methods
            .submitPriceQuote(
              bridgeNode.pastelId,
              serviceRequestId,
              quotedPrice
            )
            .accounts({
              priceQuoteSubmissionAccount: priceQuotePDA,
              bridgeContractState: bridgeContractState.publicKey,
              tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
              user: bridgeNode.keypair.publicKey,
              systemProgram: web3.SystemProgram.programId,
              bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
              bestPriceQuoteAccount,
            })
            .preInstructions([modifyComputeBudgetIx])
            .signers([bridgeNode.keypair])
            .rpc();

          await confirmTransaction(tx);
          console.log(
            `Bridge node ${
              bridgeNode.pastelId
            } submitted quote: ${quotedPrice.toString()} lamports`
          );

          // Verify the quote was stored
          const bestQuoteAccount =
            await program.account.bestPriceQuoteReceivedForServiceRequest.fetch(
              bestPriceQuoteAccount
            );
          assert(
            bestQuoteAccount.bestQuoteSelectionStatus !== "noQuotesReceivedYet"
          );
        } catch (error) {
          handleProgramError(error, "Price Quote Submission");
        }

        await sleep(250); // Add delay between quotes
      }
    });

    it("Selected bridge node submits Pastel txid", async () => {
      await sleep(250); // Add delay between transactions

      const [txidMappingPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("service_request_txid_map")],
        program.programId
      );

      const [bridgeNodeDataAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_nodes_data")],
        program.programId
      );

      // Get the best quote to find selected bridge node
      const bestQuoteAccount =
        await program.account.bestPriceQuoteReceivedForServiceRequest.fetch(
          bestPriceQuoteAccount
        );

      const selectedNode = bridgeNodes.find(
        (node) => node.pastelId === bestQuoteAccount.bestBridgeNodePastelId
      );

      if (!selectedNode) {
        throw new Error("Selected bridge node not found");
      }

      const pastelTxid = "0x" + crypto.randomBytes(32).toString("hex");

      try {
        const modifyComputeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
          units: COMPUTE_UNITS_PER_TX,
        });

        const tx = await program.methods
          .submitPastelTxid(serviceRequestId, pastelTxid)
          .accounts({
            serviceRequestSubmissionAccount: serviceRequestPDA,
            bridgeContractState: bridgeContractState.publicKey,
            bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
            serviceRequestTxidMappingDataAccount: txidMappingPDA,
            systemProgram: web3.SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .signers([selectedNode.keypair])
          .rpc();

        await confirmTransaction(tx);
        console.log("Pastel txid submitted:", pastelTxid);

        // Verify the txid was stored
        const txidMapping =
          await program.account.serviceRequestTxidMappingDataAccount.fetch(
            txidMappingPDA
          );
        const mapping = txidMapping.mappings.find(
          (m) => m.serviceRequestId === serviceRequestId
        );
        assert(mapping, "TXID mapping should be stored");
        assert.equal(
          mapping.pastelTxid,
          pastelTxid,
          "Pastel TXID should match"
        );
      } catch (error) {
        handleProgramError(error, "Pastel TXID Submission");
      }
    });

    it("Processes oracle consensus data and releases payment", async () => {
      await sleep(250); // Add delay between transactions

      const [bridgeEscrowAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_escrow_account")],
        program.programId
      );

      const [bridgeRewardPoolAccountPDA] =
        await PublicKey.findProgramAddressSync(
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

      const [txidMappingPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("service_request_txid_map")],
        program.programId
      );

      try {
        // First add mock consensus data
        const txidMapping =
          await program.account.serviceRequestTxidMappingDataAccount.fetch(
            txidMappingPDA
          );
        const mapping = txidMapping.mappings.find(
          (m) => m.serviceRequestId === serviceRequestId
        );

        if (!mapping) {
          throw new Error("TXID mapping not found");
        }

        // Now process the oracle data
        const modifyComputeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
          units: COMPUTE_UNITS_PER_TX,
        });

        const tx = await program.methods
          .processOracleData(mapping.pastelTxid, serviceRequestId)
          .accounts({
            aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
            serviceRequestTxidMappingDataAccount: txidMappingPDA,
            tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
            bridgeContractState: bridgeContractState.publicKey,
            bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
            bridgeRewardPoolAccount: bridgeRewardPoolAccountPDA,
            bridgeEscrowAccount: bridgeEscrowAccountPDA,
            userAccount: userKeypair.publicKey,
            systemProgram: web3.SystemProgram.programId,
          })
          .preInstructions([modifyComputeBudgetIx])
          .rpc();

        await confirmTransaction(tx);
        console.log("Oracle data processed and payment released");

        // Verify request status
        const tempRequests =
          await program.account.tempServiceRequestsDataAccount.fetch(
            tempServiceRequestsDataAccountPDA
          );
        const request = tempRequests.serviceRequests.find(
          (r) => r.serviceRequestId === serviceRequestId
        );
        assert(request, "Service request should exist");
        assert(request.status.completed, "Request should be completed");
      } catch (error) {
        handleProgramError(error, "Oracle Data Processing");
      }
    });
  });
});

