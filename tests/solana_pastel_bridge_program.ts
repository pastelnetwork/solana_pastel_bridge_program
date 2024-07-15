import * as anchor from "@coral-xyz/anchor";
import { Program, web3, AnchorProvider, BN } from "@coral-xyz/anchor";

import {
  SolanaPastelBridgeProgram,
  IDL,
} from "../target/types/solana_pastel_bridge_program";
import { assert } from "chai";
import * as crypto from "crypto";

const { PublicKey, SystemProgram, Keypair, Transaction } = anchor.web3;

process.env.ANCHOR_PROVIDER_URL = "http://127.0.0.1:8899";
process.env.RUST_LOG =
  "solana_runtime::system_instruction_processor=trace,solana_runtime::message_processor=trace,solana_bpf_loader=debug,solana_rbpf=debug";

const provider = AnchorProvider.env();
anchor.setProvider(provider);

const programID = new anchor.web3.PublicKey(
  "Ew8ohkPJ3JnWoZ3MWvkn86wYMRJkS385Bsis9TwQJo79"
);

// Load the IDL and the program
const program = anchor.workspace
  .SolanaPastelBridgeProgram as Program<SolanaPastelBridgeProgram>;

// Use the provider's wallet
const wallet = provider.wallet;
const adminPublicKey = wallet.publicKey;

console.log("Admin Public Key: ", adminPublicKey.toBase58());

const bridgeContractState = web3.Keypair.generate();

let bridgeNodes = [];

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
const baselinePriceUSD = 3; // Maximum price in USD
const solToUsdRate = 130; // 1 SOL = $130
const baselinePriceSol = baselinePriceUSD / solToUsdRate; // Convert USD to SOL

// Helper function to generate random price quote around baseline
function generateRandomPriceQuote(baselinePriceLamports: number): BN {
  const randomIncrement =
    Math.floor(Math.random() * (baselinePriceLamports / 10)) -
    baselinePriceLamports / 20;
  return new BN(baselinePriceLamports + randomIncrement);
}

const ErrorCodeMap = {
  0x0: "BridgeNodeAlreadyRegistered",
  0x1: "UnregisteredBridgeNode",
  0x2: "InvalidServiceRequest",
  0x3: "EscrowNotFunded",
  0x4: "InvalidFileSize",
  0x5: "InvalidServiceType",
  0x6: "QuoteExpired",
  0x7: "BridgeNodeInactive",
  0x8: "InsufficientEscrowFunds",
  0x9: "DuplicateServiceRequestId",
  0xa: "ContractPaused",
  0xb: "InvalidFileHash",
  0xc: "InvalidIpfsCid",
  0xd: "InvalidUserSolAddress",
  0xe: "InvalidRequestStatus",
  0xf: "InvalidPaymentInEscrow",
  0x10: "InvalidInitialFieldValues",
  0x11: "InvalidEscrowOrFeeAmounts",
  0x12: "InvalidServiceRequestId",
  0x13: "InvalidPastelTxid",
  0x14: "BridgeNodeNotSelected",
  0x15: "UnauthorizedBridgeNode",
  0x16: "ContractNotInitialized",
  0x17: "MappingNotFound",
  0x18: "TxidMismatch",
  0x19: "OutdatedConsensusData",
  0x1a: "ServiceRequestNotFound",
  0x1b: "ServiceRequestNotPending",
  0x1c: "QuoteResponseTimeExceeded",
  0x1d: "BridgeNodeBanned",
  0x1e: "InvalidQuotedPrice",
  0x1f: "InvalidQuoteStatus",
  0x20: "BridgeNodeScoreTooLow",
  0x21: "TxidNotFound",
  0x22: "TxidMappingNotFound",
  0x23: "InsufficientRegistrationFee",
  0x24: "UnauthorizedWithdrawalAccount",
  0x25: "InsufficientFunds",
  0x26: "TimestampConversionError",
  0x27: "RegistrationFeeNotPaid",
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
console.log("Admin ID:", adminPublicKey.toString());

describe("Solana Pastel Bridge Program Tests", () => {
  it("Initializes and expands the bridge contract state", async () => {
    try {
      console.log("Starting PDA generation...");

      const [rewardPoolAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_reward_pool_account")],
          program.programId
        );
      console.log("RewardPoolAccount PDA:", rewardPoolAccountPDA.toBase58());

      const [feeReceivingContractAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_escrow_account")],
          program.programId
        );
      console.log(
        "FeeReceivingContractAccount PDA:",
        feeReceivingContractAccountPDA.toBase58()
      );

      const [bridgeNodeDataAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("bridge_nodes_data")],
          program.programId
        );
      console.log(
        "BridgeNodeDataAccount PDA:",
        bridgeNodeDataAccountPDA.toBase58()
      );

      const [serviceRequestTxidMappingDataAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("service_request_txid_map")],
          program.programId
        );
      console.log(
        "ServiceRequestTxidMappingDataAccount PDA:",
        serviceRequestTxidMappingDataAccountPDA.toBase58()
      );

      const [aggregatedConsensusDataAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("aggregated_consensus_data")],
          program.programId
        );
      console.log(
        "AggregatedConsensusDataAccount PDA:",
        aggregatedConsensusDataAccountPDA.toBase58()
      );

      const [tempServiceRequestsDataAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("temp_service_requests_data")],
          program.programId
        );
      console.log(
        "TempServiceRequestsDataAccount PDA:",
        tempServiceRequestsDataAccountPDA.toBase58()
      );

      const [regFeeReceivingAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("reg_fee_receiving_account")],
          program.programId
        );
      console.log(
        "RegFeeReceivingAccount PDA:",
        regFeeReceivingAccountPDA.toBase58()
      );

      const minBalanceForRentExemption =
        await provider.connection.getMinimumBalanceForRentExemption(100 * 1024); // 100KB
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
      await provider.sendAndConfirm(fundTx, []);
      console.log("Bridge Contract State account funded successfully.");

      await program.methods
        .initialize(adminPublicKey)
        .accounts({
          bridgeContractState: bridgeContractState.publicKey,
          bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
          user: adminPublicKey,
          bridgeRewardPoolAccount: rewardPoolAccountPDA,
          bridgeEscrowAccount: feeReceivingContractAccountPDA,
          tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
          serviceRequestTxidMappingDataAccount:
            serviceRequestTxidMappingDataAccountPDA,
          aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
          regFeeReceivingAccount: regFeeReceivingAccountPDA,
          systemProgram: web3.SystemProgram.programId,
        })
        .signers([bridgeContractState])
        .rpc();
      console.log("Bridge Contract State initialized successfully.");

      let state = await program.account.bridgeContractState.fetch(
        bridgeContractState.publicKey
      );
      console.log("Bridge Contract State fetched.");

      assert.ok(
        state.isInitialized,
        "Bridge Contract State should be initialized after first init"
      );
      assert.equal(
        state.adminPubkey.toString(),
        adminPublicKey.toString(),
        "Admin public key should match after first init"
      );
      console.log("Initial assertions passed.");

      let currentSize = 10_240;

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
            adminPubkey: adminPublicKey,
            tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
            bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
            serviceRequestTxidMappingDataAccount:
              serviceRequestTxidMappingDataAccountPDA,
            aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
            systemProgram: web3.SystemProgram.programId,
          })
          .rpc();

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

  it("Registers new bridge nodes", async () => {
    const [bridgeRewardPoolAccountPDA] = await PublicKey.findProgramAddressSync(
      [Buffer.from("bridge_reward_pool_account")],
      program.programId
    );

    const [bridgeNodeDataAccountPDA] = await PublicKey.findProgramAddressSync(
      [Buffer.from("bridge_nodes_data")],
      program.programId
    );

    const [regFeeReceivingAccountPDA] = await PublicKey.findProgramAddressSync(
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
        console.log(`Funding bridge node ${i + 1} with registration fee...`);
        const fundTx = new Transaction().add(
          SystemProgram.transfer({
            fromPubkey: adminPublicKey,
            toPubkey: bridgeNode.publicKey,
            lamports:
              REGISTRATION_ENTRANCE_FEE_SOL * web3.LAMPORTS_PER_SOL + 10000000,
          })
        );
        await provider.sendAndConfirm(fundTx, []);
        console.log(`Bridge node ${i + 1} funded successfully.`);

        const uniquePastelId = crypto.randomBytes(32).toString("hex");
        const uniquePslAddress = "P" + crypto.randomBytes(33).toString("hex");

        console.log(`bridgeNode public key: ${bridgeNode.publicKey}`);
        console.log(`uniquePastelId: ${uniquePastelId}`);
        console.log(`uniquePslAddress: ${uniquePslAddress}`);

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
        await provider.sendAndConfirm(transferTx, [bridgeNode]);
        console.log(
          `Registration fee transferred successfully for bridge node ${i + 1}.`
        );

        console.log(`Registering bridge node ${i + 1}...`);
        await program.methods
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
        console.log(
          `Bridge Node ${i + 1} registered successfully with the address:`,
          bridgeNode.publicKey.toBase58(),
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
        console.log(`bridgeNode: ${JSON.stringify(bridgeNode)}`);
        console.log(`bridgeNode.publicKey: ${bridgeNode.publicKey}`);
        throw error;
      }
    }

    try {
      const bridgeNodeData = await program.account.bridgeNodesDataAccount.fetch(
        bridgeNodeDataAccountPDA
      );
      console.log(
        "Total number of registered bridge nodes in BridgeNodesDataAccount:",
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
    } catch (error) {
      console.error("Error fetching and verifying bridge node data:", error);
      throw error;
    }
  });

  const serviceRequestIds: string[] = [];

  it("Submits service requests", async () => {
    const [tempServiceRequestsDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("temp_service_requests_data")],
        program.programId
      );
    console.log(
      "Found PDA for TempServiceRequestsDataAccount:",
      tempServiceRequestsDataAccountPDA.toString()
    );

    const [aggregatedConsensusDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("aggregated_consensus_data")],
        program.programId
      );
    console.log(
      "Found PDA for AggregatedConsensusDataAccount:",
      aggregatedConsensusDataAccountPDA.toString()
    );

    const COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING = 0.0001;
    const lamports =
      web3.LAMPORTS_PER_SOL * COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING;

    const ADDITIONAL_SOL_FOR_ACTUAL_REQUEST = 1;
    const totalFundingLamports =
      lamports + web3.LAMPORTS_PER_SOL * ADDITIONAL_SOL_FOR_ACTUAL_REQUEST;

    for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
      console.log(
        `Generating service request ${
          i + 1
        } of ${NUMBER_OF_SIMULATED_SERVICE_REQUESTS}`
      );

      const endUserKeypair = web3.Keypair.generate();
      console.log(
        `Generated new Keypair for end user: ${endUserKeypair.publicKey.toString()}`
      );

      const transferTransaction = new web3.Transaction().add(
        web3.SystemProgram.transfer({
          fromPubkey: adminPublicKey,
          toPubkey: endUserKeypair.publicKey,
          lamports: totalFundingLamports,
        })
      );
      await provider.sendAndConfirm(transferTransaction, []);
      console.log(
        `Funded end user account with ${totalFundingLamports} lamports`
      );

      const fileHash = crypto
        .createHash("sha3-256")
        .update(`file${i}`)
        .digest("hex")
        .substring(0, 6);
      console.log(`Generated file hash for file${i}: ${fileHash}`);

      const pastelTicketTypeString =
        Object.keys(PastelTicketTypeEnum)[
          i % Object.keys(PastelTicketTypeEnum).length
        ];
      console.log(`Selected PastelTicketType: ${pastelTicketTypeString}`);

      const ipfsCid = `Qm${crypto.randomBytes(44).toString("hex")}`;
      console.log(`Generated IPFS CID: ${ipfsCid}`);

      const fileSizeBytes = Math.floor(Math.random() * 1000000) + 1;
      console.log(`Generated random file size: ${fileSizeBytes} bytes`);

      const concatenatedStr =
        pastelTicketTypeString + fileHash + endUserKeypair.publicKey.toString();
      const expectedServiceRequestIdHash = crypto
        .createHash("sha256")
        .update(concatenatedStr)
        .digest("hex");
      const expectedServiceRequestId = expectedServiceRequestIdHash.substring(
        0,
        24
      ); // Adjusted length to 12 bytes hex (24 chars)
      console.log(`Expected service request ID: ${expectedServiceRequestId}`);

      // Derive the service_request_submission_account PDA
      const [serviceRequestSubmissionAccountPDA] =
        await web3.PublicKey.findProgramAddressSync(
          [Buffer.from("srq"), Buffer.from(expectedServiceRequestId)],
          program.programId
        );
      console.log(
        `Derived service request submission account PDA: ${serviceRequestSubmissionAccountPDA.toString()}`
      );

      await program.methods
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
          user: endUserKeypair.publicKey, // End user keypair as user
          systemProgram: web3.SystemProgram.programId,
        })
        .signers([endUserKeypair]) // Only endUserKeypair is needed as a signer
        .rpc();
      console.log(`Service request ${i + 1} submitted successfully`);

      const serviceRequestSubmissionData =
        await program.account.serviceRequestSubmissionAccount.fetch(
          serviceRequestSubmissionAccountPDA
        );
      const actualServiceRequestId =
        serviceRequestSubmissionData.serviceRequest.serviceRequestId;
      console.log(`Actual service request ID: ${actualServiceRequestId}`);

      assert.equal(
        actualServiceRequestId,
        expectedServiceRequestId,
        `Service Request ID should match the expected value for request ${
          i + 1
        }`
      );

      serviceRequestIds.push(expectedServiceRequestId); // Add this line to push the IDs into the array
    }

    const tempServiceRequestsData =
      await program.account.tempServiceRequestsDataAccount.fetch(
        tempServiceRequestsDataAccountPDA
      );
    console.log(
      "Fetched TempServiceRequestsDataAccount:",
      tempServiceRequestsDataAccountPDA.toString()
    );
    console.log(
      "Total number of submitted service requests in TempServiceRequestsDataAccount:",
      tempServiceRequestsData.serviceRequests.length
    );

    serviceRequestIds.forEach((serviceRequestId, index) => {
      const isSubmitted = tempServiceRequestsData.serviceRequests.some(
        (sr) => sr.serviceRequestId === serviceRequestId
      );
      console.log(
        `Verifying service request ${
          index + 1
        } with ID: ${serviceRequestId} - ${isSubmitted ? "Found" : "Not Found"}`
      );
      assert.isTrue(
        isSubmitted,
        `Service Request ${
          index + 1
        } should be submitted in TempServiceRequestsDataAccount`
      );
    });
  });

  it("Submits price quotes for service requests", async () => {
    const [tempServiceRequestsDataAccountPDA] = await PublicKey.findProgramAddressSync(
      [Buffer.from("temp_service_requests_data")],
      program.programId
    );
  
    const [bridgeNodesDataAccountPDA] = await PublicKey.findProgramAddressSync(
      [Buffer.from("bridge_nodes_data")],
      program.programId
    );
  
    for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
      const serviceRequestId = serviceRequestIds[i];
      const truncatedServiceRequestId = serviceRequestId.substring(0, 24);
      console.log(`Processing service request ${i + 1}: ${serviceRequestId}`);
  
      const [serviceRequestSubmissionAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("srq"), Buffer.from(truncatedServiceRequestId)],
        program.programId
      );
  
      const serviceRequestSubmissionData = await program.account.serviceRequestSubmissionAccount.fetch(
        serviceRequestSubmissionAccountPDA
      );
  
      const fileSizeBytes = new BN(
        serviceRequestSubmissionData.serviceRequest.fileSizeBytes
      ).toNumber();
      console.log(`File size for service request ${i + 1}: ${fileSizeBytes} bytes`);
  
      const baselinePriceLamports = Math.floor(
        (fileSizeBytes / 1000000) * baselinePriceSol * 1e9
      );
      console.log(`Baseline price for service request ${i + 1}: ${baselinePriceLamports} lamports`);
  
      const [bestPriceQuoteAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("bpx"), Buffer.from(truncatedServiceRequestId)],
        program.programId
      );
  
      await program.methods
        .initializeBestPriceQuote(truncatedServiceRequestId)
        .accounts({
          bestPriceQuoteAccount: bestPriceQuoteAccountPDA,
          user: provider.wallet.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .rpc();
      console.log(`Best price quote account initialized for service request ${i + 1}`);
  
      for (let j = 0; j < bridgeNodes.length; j++) {
        const bridgeNode = bridgeNodes[j];
        if (!bridgeNode || !bridgeNode.keypair || !bridgeNode.keypair.publicKey) {
          console.error(`Bridge node ${j + 1} is not defined or does not have a publicKey`);
          continue;
        }
  
        const quotedPriceLamports = generateRandomPriceQuote(baselinePriceLamports);
        console.log(`Submitting price quote from bridge node ${j + 1}: ${bridgeNode.keypair.publicKey.toBase58()}`);
        console.log(`Quoted price: ${quotedPriceLamports.toString()} lamports`);
  
        const [priceQuoteSubmissionAccountPDA] = await PublicKey.findProgramAddressSync(
          [Buffer.from("px_quote"), Buffer.from(truncatedServiceRequestId)],
          program.programId
        );
  
        console.log(`Derived priceQuoteSubmissionAccountPDA: ${priceQuoteSubmissionAccountPDA.toBase58()}`);
  
        console.log("Account parameters for submitPriceQuote:");
        console.log(`priceQuoteSubmissionAccount: ${priceQuoteSubmissionAccountPDA.toBase58()}`);
        console.log(`bridgeContractState: ${bridgeContractState.publicKey.toBase58()}`);
        console.log(`tempServiceRequestsDataAccount: ${tempServiceRequestsDataAccountPDA.toBase58()}`);
        console.log(`user: ${bridgeNode.keypair.publicKey.toBase58()}`);
        console.log(`bridgeNodesDataAccount: ${bridgeNodesDataAccountPDA.toBase58()}`);
        console.log(`bestPriceQuoteAccount: ${bestPriceQuoteAccountPDA.toBase58()}`);
  
        console.log("Method parameters for submitPriceQuote:");
        console.log(`bridgeNode.pastelId: ${bridgeNode.pastelId}`);
        console.log(`truncatedServiceRequestId: ${truncatedServiceRequestId}`);
        console.log(`serviceRequestId: ${serviceRequestId}`);
        console.log(`quotedPriceLamports: ${quotedPriceLamports.toString()}`);
  
        try {
          await program.methods
            .submitPriceQuote(
              bridgeNode.pastelId,
              serviceRequestId,
              quotedPriceLamports
            )
            .accounts({
              priceQuoteSubmissionAccount: priceQuoteSubmissionAccountPDA,
              bridgeContractState: bridgeContractState.publicKey,
              tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
              user: bridgeNode.keypair.publicKey,
              bridgeNodesDataAccount: bridgeNodesDataAccountPDA,
              bestPriceQuoteAccount: bestPriceQuoteAccountPDA,
              systemProgram: SystemProgram.programId,
            })
            .signers([bridgeNode.keypair])
            .rpc();
  
          console.log(`Price quote for service request ${i + 1} submitted successfully by bridge node ${bridgeNode.keypair.publicKey.toBase58()}`);
        } catch (error) {
          console.error(`Error submitting price quote for service request ${i + 1}:`, error);
          if (error instanceof anchor.AnchorError) {
            console.error(`Error code: ${error.error.errorCode.number}`);
            console.error(`Error message: ${error.error.errorMessage}`);
          }
          throw error;
        }
      }
    }
  
    for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
      const truncatedServiceRequestId = serviceRequestIds[i].substring(0, 24);
  
      const [bestPriceQuoteAccountPDA] = await PublicKey.findProgramAddressSync(
        [Buffer.from("bpx"), Buffer.from(truncatedServiceRequestId)],
        program.programId
      );
  
      const bestPriceQuoteData = await program.account.bestPriceQuoteReceivedForServiceRequest.fetch(
        bestPriceQuoteAccountPDA
      );
      console.log(`Best price quotes received for service request ${i + 1}:`, bestPriceQuoteData);
  
      const bestQuote = bestPriceQuoteData.serviceRequestId === serviceRequestIds[i];
      assert.isTrue(
        bestQuote,
        `Best price quote should be selected for service request ${i + 1}`
      );
    }
  });
  
});

//   // Submit Pastel TxIDs
//   it("Submits Pastel TxIDs for service requests", async () => {
//     // Find the PDA for the TempServiceRequestsDataAccount
//     const [tempServiceRequestsDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
//       [Buffer.from("temp_service_requests_data")],
//       program.programId
//     );

//     // Find the PDA for the ServiceRequestTxidMappingDataAccount
//     const [serviceRequestTxidMappingDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
//       [Buffer.from("service_request_txid_map")],
//       program.programId
//     );

//     for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
//       const serviceRequestId = serviceRequestIds[i];
//       const bridgeNode = bridgeNodes[i % bridgeNodes.length];
//       const pastelTxid = crypto.randomBytes(32).toString('hex'); // Generate a random Pastel TxID

//       // Submit the Pastel TxID
//       await program.methods
//         .submitPastelTxid(serviceRequestId, pastelTxid)
//         .accounts({
//           serviceRequestSubmissionAccount: web3.Keypair.generate().publicKey,
//           bridgeContractState: bridgeContractState.publicKey,
//           bridgeNodesDataAccount: web3.Keypair.generate().publicKey,
//           serviceRequestTxidMappingDataAccount: serviceRequestTxidMappingDataAccountPDA,
//           systemProgram: web3.SystemProgram.programId,
//         })
//         .signers([bridgeNode])
//         .rpc();

//       console.log(`Pastel TxID for service request ${i + 1} submitted successfully by bridge node ${bridgeNode.publicKey.toBase58()}`);
//     }

//     // Fetch the ServiceRequestTxidMappingDataAccount to verify the TxIDs are submitted
//     const serviceRequestTxidMappingData = await program.account.serviceRequestTxidMappingDataAccount.fetch(
//       serviceRequestTxidMappingDataAccountPDA
//     );
//     console.log(
//       "Total number of submitted Pastel TxIDs in ServiceRequestTxidMappingDataAccount:",
//       serviceRequestTxidMappingData.mappings.length
//     );

//     // Verify each Pastel TxID is submitted in ServiceRequestTxidMappingDataAccount
//     serviceRequestIds.forEach((serviceRequestId, index) => {
//       const isSubmitted = serviceRequestTxidMappingData.mappings.some((mapping) =>
//         mapping.serviceRequestId === serviceRequestId
//       );
//       assert.isTrue(
//         isSubmitted,
//         `Pastel TxID for Service Request ${
//           index + 1
//         } should be submitted in ServiceRequestTxidMappingDataAccount`
//       );
//     });
//   });

//   const adminKeypair = (provider.wallet as anchor.Wallet).payer;

//   // Access Oracle Data
//   it("Accesses Oracle data and handles post-transaction tasks", async () => {
//     // Find the PDA for the AggregatedConsensusDataAccount
//     const [aggregatedConsensusDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
//       [Buffer.from("aggregated_consensus_data")],
//       program.programId
//     );

//     // Find the PDA for the ServiceRequestTxidMappingDataAccount
//     const [serviceRequestTxidMappingDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
//       [Buffer.from("service_request_txid_map")],
//       program.programId
//     );

//     // Fetch aggregated consensus data for each service request
//     for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
//       const serviceRequestId = serviceRequestIds[i];
//       const txid = trackedTxids[i];

//       // Access the Oracle data
//       await program.methods
//         .processOracleData(txid, serviceRequestId)
//         .accounts({
//           aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
//           serviceRequestTxidMappingDataAccount: serviceRequestTxidMappingDataAccountPDA,
//           tempServiceRequestsDataAccount: web3.Keypair.generate().publicKey,
//           bridgeContractState: bridgeContractState.publicKey,
//           bridgeNodesDataAccount: web3.Keypair.generate().publicKey,
//           bridgeRewardPoolAccount: web3.Keypair.generate().publicKey,
//           bridgeEscrowAccount: web3.Keypair.generate().publicKey,
//           userAccount: admin.publicKey,
//           systemProgram: web3.SystemProgram.programId,
//         })
//         .signers([adminKeypair]) // Use the payer from the provider's wallet
//         .rpc();

//       console.log(`Oracle data accessed and post-transaction tasks handled for service request ${i + 1} and TxID ${txid}`);
//     }
//   });

// });
