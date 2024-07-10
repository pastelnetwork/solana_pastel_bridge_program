import * as anchor from "@coral-xyz/anchor";
import { Program, web3, AnchorProvider } from "@coral-xyz/anchor";

import { SolanaPastelBridgeProgram, IDL } from "../target/types/solana_pastel_bridge_program";
import { assert } from "chai";
import * as crypto from "crypto";

const { PublicKey, SystemProgram, Keypair, Transaction } = anchor.web3;

process.env.ANCHOR_PROVIDER_URL = "http://127.0.0.1:8899";
process.env.RUST_LOG = "solana_runtime::system_instruction_processor=trace,solana_runtime::message_processor=trace,solana_bpf_loader=debug,solana_rbpf=debug";

const provider = AnchorProvider.env();
anchor.setProvider(provider);

const programID = new anchor.web3.PublicKey("Ew8ohkPJ3JnWoZ3MWvkn86wYMRJkS385Bsis9TwQJo79");
const program = new Program<SolanaPastelBridgeProgram>(IDL, programID, provider);

// Extract keypair from provider's wallet
const admin = Keypair.fromSecretKey(Uint8Array.from(provider.wallet.payer.secretKey));
const bridgeContractState = web3.Keypair.generate();

let bridgeNodes = []; // Array to store bridge node keypairs

const maxSize = 100 * 1024; // 100KB (max size of the bridge contract state account)

const NUM_BRIDGE_NODES = 12;

const REGISTRATION_ENTRANCE_FEE_SOL = 0.1;

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
  it("Initializes and expands the bridge contract state", async () => {
    try {
      console.log("Starting PDA generation...");

      const [rewardPoolAccountPDA] = await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_reward_pool_account")],
        program.programId
      );
      console.log("RewardPoolAccount PDA:", rewardPoolAccountPDA.toBase58());

      const [feeReceivingContractAccountPDA] = await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_escrow_account")],
        program.programId
      );
      console.log("FeeReceivingContractAccount PDA:", feeReceivingContractAccountPDA.toBase58());

      const [bridgeNodeDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("bridge_nodes_data")],
        program.programId
      );
      console.log("BridgeNodeDataAccount PDA:", bridgeNodeDataAccountPDA.toBase58());

      const [serviceRequestTxidMappingDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("service_request_txid_map")],
        program.programId
      );
      console.log("ServiceRequestTxidMappingDataAccount PDA:", serviceRequestTxidMappingDataAccountPDA.toBase58());

      const [aggregatedConsensusDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("aggregated_consensus_data")],
        program.programId
      );
      console.log("AggregatedConsensusDataAccount PDA:", aggregatedConsensusDataAccountPDA.toBase58());

      const [tempServiceRequestsDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("temp_service_requests_data")],
        program.programId
      );
      console.log("TempServiceRequestsDataAccount PDA:", tempServiceRequestsDataAccountPDA.toBase58());

      const [regFeeReceivingAccountPDA] = await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("reg_fee_receiving_account")],
        program.programId
      );
      console.log("RegFeeReceivingAccount PDA:", regFeeReceivingAccountPDA.toBase58());

      const minBalanceForRentExemption = await provider.connection.getMinimumBalanceForRentExemption(100 * 1024); // 100KB
      console.log("Minimum Balance for Rent Exemption:", minBalanceForRentExemption);

      const fundTx = new anchor.web3.Transaction().add(
        anchor.web3.SystemProgram.transfer({
          fromPubkey: admin.publicKey,
          toPubkey: bridgeContractState.publicKey,
          lamports: minBalanceForRentExemption,
        })
      );
      await provider.sendAndConfirm(fundTx, [admin]);
      console.log("Bridge Contract State account funded successfully.");

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
          regFeeReceivingAccount: regFeeReceivingAccountPDA,
          systemProgram: web3.SystemProgram.programId,
        })
        .signers([bridgeContractState])
        .rpc();
      console.log("Bridge Contract State initialized successfully.");

      let state = await program.account.bridgeContractState.fetch(bridgeContractState.publicKey);
      console.log("Bridge Contract State fetched.");

      assert.ok(state.isInitialized, "Bridge Contract State should be initialized after first init");
      assert.equal(state.adminPubkey.toString(), admin.publicKey.toString(), "Admin public key should match after first init");
      console.log("Initial assertions passed.");

      let currentSize = 10_240; // Initial size after first init

      while (currentSize < maxSize) {
        console.log(`Expanding Bridge Contract State size from ${currentSize} to ${currentSize + 10_240}`);
        await program.methods
          .reallocateBridgeState()
          .accounts({
            bridgeContractState: bridgeContractState.publicKey,
            adminPubkey: admin.publicKey,
            tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
            bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
            serviceRequestTxidMappingDataAccount: serviceRequestTxidMappingDataAccountPDA,
            aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
            systemProgram: web3.SystemProgram.programId,
          })
          .rpc();

        currentSize += 10_240;
        state = await program.account.bridgeContractState.fetch(bridgeContractState.publicKey);

        console.log(`Bridge Contract State size after expansion: ${currentSize}`);
      }

      assert.equal(currentSize, maxSize, "Bridge Contract State should reach the maximum size");
      console.log("Bridge Contract State expanded to the maximum size successfully");

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
      console.log(`Bridge Node ${i + 1} Keypair:`, bridgeNode.publicKey.toBase58());

      try {
        console.log(`Funding bridge node ${i + 1} with registration fee...`);
        const fundTx = new Transaction().add(
          SystemProgram.transfer({
            fromPubkey: admin.publicKey,
            toPubkey: bridgeNode.publicKey,
            lamports: REGISTRATION_ENTRANCE_FEE_SOL * web3.LAMPORTS_PER_SOL + 10000000,
          })
        );
        await provider.sendAndConfirm(fundTx, [admin]);
        console.log(`Bridge node ${i + 1} funded successfully.`);

        const uniquePastelId = crypto.randomBytes(32).toString('hex');
        const uniquePslAddress = 'P' + crypto.randomBytes(33).toString('hex');

        console.log(`bridgeNode public key: ${bridgeNode.publicKey}`);
        console.log(`uniquePastelId: ${uniquePastelId}`);
        console.log(`uniquePslAddress: ${uniquePslAddress}`);

        console.log(`Transferring registration fee from bridge node ${i + 1} to fee receiving contract account...`);
        const transferTx = new Transaction().add(
          SystemProgram.transfer({
            fromPubkey: bridgeNode.publicKey,
            toPubkey: regFeeReceivingAccountPDA,
            lamports: REGISTRATION_ENTRANCE_FEE_SOL * web3.LAMPORTS_PER_SOL,
          })
        );
        await provider.sendAndConfirm(transferTx, [bridgeNode]);
        console.log(`Registration fee transferred successfully for bridge node ${i + 1}.`);

        console.log(`Registering bridge node ${i + 1}...`);
        await program.methods
          .registerNewBridgeNode(uniquePastelId, uniquePslAddress)
          .accounts({
            bridgeNodesDataAccount: bridgeNodeDataAccountPDA,
            user: bridgeNode.publicKey,
            bridgeRewardPoolAccount: bridgeRewardPoolAccountPDA,
            regFeeReceivingAccount: regFeeReceivingAccountPDA, // Corrected account name
            systemProgram: SystemProgram.programId,
          })
          .signers([bridgeNode])
          .rpc();
        console.log(`Bridge Node ${i + 1} registered successfully with the address:`, bridgeNode.publicKey.toBase58(), `Pastel ID: ${uniquePastelId}`, `PSL Address: ${uniquePslAddress}`);
        bridgeNodes.push({ keypair: bridgeNode, pastelId: uniquePastelId, pslAddress: uniquePslAddress });
      } catch (error) {
        console.error(`Error registering bridge node ${i + 1}:`, error);
        console.log(`bridgeNode: ${JSON.stringify(bridgeNode)}`);
        console.log(`bridgeNode.publicKey: ${bridgeNode.publicKey}`);
        throw error;
      }
    }

    try {
      const bridgeNodeData = await program.account.bridgeNodesDataAccount.fetch(bridgeNodeDataAccountPDA);
      console.log("Total number of registered bridge nodes in BridgeNodesDataAccount:", bridgeNodeData.bridgeNodes.length);

      bridgeNodes.forEach((bridgeNode, index) => {
        const isRegistered = bridgeNodeData.bridgeNodes.some(
          (bn) =>
            bn.rewardAddress.equals(bridgeNode.keypair.publicKey) &&
            bn.pastelId === bridgeNode.pastelId &&
            bn.bridgeNodePslAddress === bridgeNode.pslAddress
        );
        assert.isTrue(isRegistered, `Bridge Node ${index + 1} should be registered in BridgeNodesDataAccount`);
      });
    } catch (error) {
      console.error("Error fetching and verifying bridge node data:", error);
      throw error;
    }
  });
});

  // // Define the serviceRequestIds array before it's used
  // const serviceRequestIds: string[] = [];

  // // Submit Service Requests
  // it("Submits service requests", async () => {
  //   // Find the PDA for the TempServiceRequestsDataAccount
  //   const [tempServiceRequestsDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
  //     [Buffer.from("temp_service_requests_data")],
  //     program.programId
  //   );

  //   // Find the PDA for the ServiceRequestSubmissionAccount
  //   const serviceRequestIds = []; // Array to store service request IDs for later use

  //   for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
  //     const serviceRequestSubmissionAccount = web3.Keypair.generate();
  //     const fileHash = crypto.createHash('sha3-256').update(`file${i}`).digest('hex').substring(0, 6);
  //     const pastelTicketTypeString = Object.keys(PastelTicketTypeEnum)[i % Object.keys(PastelTicketTypeEnum).length];
  //     const ipfsCid = `Qm${crypto.randomBytes(44).toString('hex')}`;
  //     const fileSizeBytes = Math.floor(Math.random() * 1000000) + 1; // Random file size between 1 and 1,000,000 bytes

  //     const serviceRequestId = crypto.createHash('sha256').update(pastelTicketTypeString + fileHash + admin.publicKey.toString()).digest('hex').substring(0, 32);
  //     serviceRequestIds.push(serviceRequestId);

  //     // Submit the service request
  //     await program.methods
  //       .submitServiceRequest(pastelTicketTypeString, fileHash, ipfsCid, new BN(fileSizeBytes))
  //       .accounts({
  //         serviceRequestSubmissionAccount: serviceRequestSubmissionAccount.publicKey,
  //         bridgeContractState: bridgeContractState.publicKey,
  //         tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
  //         user: admin.publicKey,
  //         systemProgram: web3.SystemProgram.programId,
  //       })
  //       .signers([serviceRequestSubmissionAccount])
  //       .rpc();

  //     console.log(`Service request ${i + 1} submitted successfully with ID: ${serviceRequestId}`);
  //   }

  //   // Fetch the TempServiceRequestsDataAccount to verify all service requests are submitted
  //   const tempServiceRequestsData = await program.account.tempServiceRequestsDataAccount.fetch(
  //     tempServiceRequestsDataAccountPDA
  //   );
  //   console.log(
  //     "Total number of submitted service requests in TempServiceRequestsDataAccount:",
  //     tempServiceRequestsData.serviceRequests.length
  //   );

  //   // Verify each service request is submitted in TempServiceRequestsDataAccount
  //   serviceRequestIds.forEach((serviceRequestId, index) => {
  //     const isSubmitted = tempServiceRequestsData.serviceRequests.some((sr) =>
  //       sr.serviceRequestId === serviceRequestId
  //     );
  //     assert.isTrue(
  //       isSubmitted,
  //       `Service Request ${
  //         index + 1
  //       } should be submitted in TempServiceRequestsDataAccount`
  //     );
  //   });
  // });
    
// // Submit Price Quotes
// it("Submits price quotes for service requests", async () => {
//   // Find the PDA for the TempServiceRequestsDataAccount
//   const [tempServiceRequestsDataAccountPDA] = await web3.PublicKey.findProgramAddressSync(
//     [Buffer.from("temp_service_requests_data")],
//     program.programId
//   );

//   // Find the PDA for the BestPriceQuoteReceivedForServiceRequest
//   const [bestPriceQuoteAccountPDA] = await web3.PublicKey.findProgramAddressSync(
//     [Buffer.from("best_price_quote_account")],
//     program.programId
//   );

//   for (let i = 0; i < NUMBER_OF_SIMULATED_SERVICE_REQUESTS; i++) {
//     const serviceRequestId = serviceRequestIds[i];
//     const bridgeNode = bridgeNodes[i % bridgeNodes.length];
//     const quotedPriceLamports = new BN(Math.floor(Math.random() * 1000000) + 1); // Random price between 1 and 1,000,000 lamports

//     // Submit the price quote
//     await program.methods
//       .submitPriceQuote(bridgeNode.publicKey.toString(), serviceRequestId, quotedPriceLamports)
//       .accounts({
//         priceQuoteSubmissionAccount: web3.Keypair.generate().publicKey,
//         bridgeContractState: bridgeContractState.publicKey,
//         tempServiceRequestsDataAccount: tempServiceRequestsDataAccountPDA,
//         user: bridgeNode.publicKey,
//         bridgeNodesDataAccount: web3.Keypair.generate().publicKey,
//         bestPriceQuoteAccount: bestPriceQuoteAccountPDA,
//         systemProgram: web3.SystemProgram.programId,
//       })
//       .signers([bridgeNode])
//       .rpc();

//     console.log(`Price quote for service request ${i + 1} submitted successfully by bridge node ${bridgeNode.publicKey.toBase58()}`);
//   }

//   // Fetch the BestPriceQuoteReceivedForServiceRequest to verify the best price quotes are selected
//   const bestPriceQuoteData = await program.account.bestPriceQuoteReceivedForServiceRequest.fetch(
//     bestPriceQuoteAccountPDA
//   );
//   console.log(
//     "Best price quotes received for service requests:",
//     bestPriceQuoteData
//   );

//   // Verify the best price quotes are selected for each service request
//   serviceRequestIds.forEach((serviceRequestId, index) => {
//     const bestQuote = bestPriceQuoteData.serviceRequestId === serviceRequestId;
//     assert.isTrue(
//       bestQuote,
//       `Best price quote should be selected for service request ${index + 1}`
//     );
//   });
// });


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