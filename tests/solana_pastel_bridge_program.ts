import * as anchor from "@coral-xyz/anchor";
import { Program, web3, AnchorProvider, BN } from "@coral-xyz/anchor";
import {
  SolanaPastelBridgeProgram,
  IDL,
} from "../target/types/solana_pastel_bridge_program";
import { assert } from "chai";
import * as crypto from "crypto";
const { ComputeBudgetProgram, Transaction } = anchor.web3;

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
const oracleContractState = web3.Keypair.generate();
let contributors = []; // Array to store contributor keypairs
let trackedTxids = []; // Initialize an empty array to track TXIDs

const maxSize = 100 * 1024; // 200KB (max size of the oracle contract state account)

const NUM_CONTRIBUTORS = 12;
const NUMBER_OF_SIMULATED_REPORTS = 20;

const REGISTRATION_ENTRANCE_FEE_SOL = 0.1;
const COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING = 0.0001;
const MIN_NUMBER_OF_ORACLES = 8;
const MIN_REPORTS_FOR_REWARD = 10;
const BAD_CONTRIBUTOR_INDEX = 5; // Define a constant to represent the index at which contributors start submitting incorrect reports with increasing probability
const MIN_COMPLIANCE_SCORE_FOR_REWARD = 65;
const MIN_RELIABILITY_SCORE_FOR_REWARD = 80;
const BASE_REWARD_AMOUNT_IN_LAMPORTS = 100000;

const ErrorCodeMap = {
  0x0: 'ContributorAlreadyRegistered',
  0x1: 'UnregisteredOracle',
  0x2: 'InvalidTxid',
  0x3: 'InvalidFileHashLength',
  0x4: 'MissingPastelTicketType',
  0x5: 'MissingFileHash',
  0x6: 'RegistrationFeeNotPaid',
  0x7: 'NotEligibleForReward',
  0x8: 'NotBridgeContractAddress',
  0x9: 'InsufficientFunds',
  0xa: 'UnauthorizedWithdrawalAccount',
  0xb: 'InvalidPaymentAmount',
  0xc: 'PaymentNotFound',
  0xd: 'PendingPaymentAlreadyInitialized',
  0xe: 'AccountAlreadyInitialized',
  0xf: 'PendingPaymentInvalidAmount',
  0x10: 'InvalidPaymentStatus',
  0x11: 'InvalidTxidStatus',
  0x12: 'InvalidPastelTicketType',
  0x13: 'ContributorNotRegistered',
  0x14: 'ContributorBanned',
  0x15: 'EnoughReportsSubmittedForTxid'
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

describe("Initialization", () => {
  it("Initializes and expands the oracle contract state", async () => {
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

    // Find the PDA for the ContributorDataAccount
    const [contributorDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("contributor_data")],
        program.programId
      );

    // Find the PDA for the TxidSubmissionCountsAccount
    const [txidSubmissionCountsAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("txid_submission_counts")],
        program.programId
      );

    // Find the PDA for the AggregatedConsensusDataAccount
    const [aggregatedConsensusDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("aggregated_consensus_data")],
        program.programId
      );

    // Find the PDA for the TempTxStatusReportAccount
    const [tempReportAccountPDA] = web3.PublicKey.findProgramAddressSync(
      [Buffer.from("temp_tx_status_report")],
      program.programId
    );

    // Calculate the rent-exempt minimum balance for the account size
    const minBalanceForRentExemption =
      await provider.connection.getMinimumBalanceForRentExemption(100 * 1024); // 100KB
    console.log(
      "Minimum Balance for Rent Exemption:",
      minBalanceForRentExemption
    );

    // Fund the oracleContractState account with enough SOL for rent exemption
    console.log("Funding Oracle Contract State account for rent exemption");
    const fundTx = new anchor.web3.Transaction().add(
      anchor.web3.SystemProgram.transfer({
        fromPubkey: admin.publicKey,
        toPubkey: oracleContractState.publicKey,
        lamports: minBalanceForRentExemption,
      })
    );
    await provider.sendAndConfirm(fundTx);

    // Initial Initialization
    console.log("Initializing Oracle Contract State");
    await program.methods
      .initialize(admin.publicKey)
      .accounts({
        oracleContractState: oracleContractState.publicKey,
        contributorDataAccount: contributorDataAccountPDA,
        user: admin.publicKey,
        rewardPoolAccount: rewardPoolAccountPDA,
        feeReceivingContractAccount: feeReceivingContractAccountPDA,
        tempReportAccount: tempReportAccountPDA,
        txidSubmissionCountsAccount: txidSubmissionCountsAccountPDA,
        aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([oracleContractState])
      .rpc();

    let state = await program.account.oracleContractState.fetch(
      oracleContractState.publicKey
    );
    assert.ok(
      state.isInitialized,
      "Oracle Contract State should be initialized after first init"
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
        `Expanding Oracle Contract State size from ${currentSize} to ${
          currentSize + 10_240
        }`
      );
      await program.methods
        .reallocateOracleState()
        .accounts({
          oracleContractState: oracleContractState.publicKey,
          adminPubkey: admin.publicKey,
          tempReportAccount: tempReportAccountPDA,
          contributorDataAccount: contributorDataAccountPDA,
          txidSubmissionCountsAccount: txidSubmissionCountsAccountPDA,
          aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
        })
        .rpc();

      currentSize += 10_240;
      state = await program.account.oracleContractState.fetch(
        oracleContractState.publicKey
      );

      // Log the updated size of the account
      console.log(`Oracle Contract State size after expansion: ${currentSize}`);
    }

    // Final Assertions
    assert.equal(
      currentSize,
      maxSize,
      "Oracle Contract State should reach the maximum size"
    );
    console.log(
      "Oracle Contract State expanded to the maximum size successfully"
    );
  });
});

describe("Set Bridge Contract", () => {
  it("Sets the bridge contract address to admin address", async () => {
    await program.methods
      .setBridgeContract(admin.publicKey)
      .accounts({
        oracleContractState: oracleContractState.publicKey,
        adminPubkey: admin.publicKey,
      })
      .rpc();

    // Fetch the updated state to verify the bridge contract address
    const state = await program.account.oracleContractState.fetch(
      oracleContractState.publicKey
    );

    // Assertions
    assert.strictEqual(
      state.bridgeContractPubkey.toString(),
      admin.publicKey.toString(),
      "The bridge contract pubkey should be set to the admin address"
    );
    console.log("Bridge contract address set to admin address");
  });
});

describe("Contributor Registration", () => {
  it("Registers new data contributors", async () => {
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

    const [contributorDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("contributor_data")],
        program.programId
      );

    for (let i = 0; i < NUM_CONTRIBUTORS; i++) {
      // Generate a new keypair for each contributor
      const contributor = web3.Keypair.generate();

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

      // Call the RPC method to register the new data contributor
      await program.methods
        .registerNewDataContributor()
        .accounts({
          contributorDataAccount: contributorDataAccountPDA,
          contributorAccount: contributor.publicKey,
          rewardPoolAccount: rewardPoolAccountPDA,
          feeReceivingContractAccount: feeReceivingContractAccountPDA,
        })
        .signers([contributor])
        .rpc();

      console.log(
        `Contributor ${i + 1} registered successfully with the address:`,
        contributor.publicKey.toBase58()
      );
      contributors.push(contributor);
    }

    // Fetch the ContributorDataAccount to verify all contributors are registered
    const contributorData = await program.account.contributorDataAccount.fetch(
      contributorDataAccountPDA
    );
    console.log(
      "Total number of registered contributors in ContributorDataAccount:",
      contributorData.contributors.length
    );

    // Verify each contributor is registered in ContributorDataAccount
    contributors.forEach((contributor, index) => {
      const isRegistered = contributorData.contributors.some((c) =>
        c.rewardAddress.equals(contributor.publicKey)
      );
      assert.isTrue(
        isRegistered,
        `Contributor ${
          index + 1
        } should be registered in ContributorDataAccount`
      );
    });
  });
});

describe("TXID Monitoring", () => {
  it("Adds multiple TXIDs for monitoring", async () => {
    // Define the number of TXIDs to add for monitoring
    const numTxids = NUMBER_OF_SIMULATED_REPORTS;

    // Helper function to generate a random TXID
    const generateRandomTxid = () => {
      return [...Array(64)]
        .map(() => Math.floor(Math.random() * 16).toString(16))
        .join("");
    };

    for (let i = 0; i < numTxids; i++) {
      const txid = generateRandomTxid();

      const expectedAmountLamports =
        COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING *
        web3.LAMPORTS_PER_SOL;
      const expectedAmountStr = expectedAmountLamports.toString();

      const preimageString =
        "pending_payment" + txid + admin.publicKey.toBase58();
      const preimageBytes = Buffer.from(preimageString, "utf8");
      const seedHash = crypto
        .createHash("sha256")
        .update(preimageBytes)
        .digest();
      const [pendingPaymentAccountPDA] = web3.PublicKey.findProgramAddressSync(
        [seedHash],
        program.programId
      );

      await program.methods
        .addPendingPayment(txid, expectedAmountStr, "Pending")
        .accounts({
          pendingPaymentAccount: pendingPaymentAccountPDA,
          oracleContractState: oracleContractState.publicKey,
          user: admin.publicKey,
          systemProgram: web3.SystemProgram.programId,
        })
        .rpc();

      await program.methods
        .addTxidForMonitoring({ txid: txid })
        .accounts({
          oracleContractState: oracleContractState.publicKey,
          caller: admin.publicKey,
          pendingPaymentAccount: pendingPaymentAccountPDA,
          user: admin.publicKey,
          systemProgram: web3.SystemProgram.programId,
        })
        .rpc();

      // Fetch the updated state
      const state = await program.account.oracleContractState.fetch(
        oracleContractState.publicKey
      );
      const pendingPaymentData =
        await program.account.pendingPaymentAccount.fetch(
          pendingPaymentAccountPDA
        );

      // Assertions for each TXID
      assert(
        state.monitoredTxids.includes(txid),
        `The TXID ${txid} should be added to the monitored list`
      );
      assert.strictEqual(
        pendingPaymentData.pendingPayment.expectedAmount.toNumber(),
        expectedAmountLamports,
        `The expected amount for pending payment for TXID ${txid} should match`
      );
      console.log(`TXID ${txid} successfully added for monitoring`);
    }
  });
});

describe("TXID Monitoring Verification", () => {
  it("Verifies all monitored TXIDs have corresponding PendingPayment structs", async () => {
    // Fetch monitored TXIDs from the updated state
    const state = await program.account.oracleContractState.fetch(
      oracleContractState.publicKey
    );
    const monitoredTxids = state.monitoredTxids;

    for (const txid of monitoredTxids) {
      // Derive the PDA for each PendingPaymentAccount
      const preimageString =
        "pending_payment" + txid + admin.publicKey.toBase58();
      const preimageBytes = Buffer.from(preimageString, "utf8");
      const seedHash = crypto
        .createHash("sha256")
        .update(preimageBytes)
        .digest();
      const [pendingPaymentAccountPDA] = web3.PublicKey.findProgramAddressSync(
        [seedHash],
        program.programId
      );

      // Fetch the PendingPayment struct for each TXID
      const pendingPaymentData =
        await program.account.pendingPaymentAccount.fetch(
          pendingPaymentAccountPDA
        );

      // Assertions to verify the PendingPayment struct is correctly initialized
      assert.strictEqual(
        pendingPaymentData.pendingPayment.txid,
        txid,
        `The TXID in PendingPayment should match the monitored TXID: ${txid}`
      );
      assert.strictEqual(
        pendingPaymentData.pendingPayment.expectedAmount.toNumber(),
        COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING *
          web3.LAMPORTS_PER_SOL,
        `The expected amount in PendingPayment should match for TXID: ${txid}`
      );

      // Convert paymentStatus to JSON and compare the stringified version
      const paymentStatusJson = JSON.stringify(
        pendingPaymentData.pendingPayment.paymentStatus
      );
      assert.strictEqual(
        paymentStatusJson,
        JSON.stringify({ pending: {} }),
        `The payment status for TXID: ${txid} should be 'Pending'`
      );
      console.log(`Verified PendingPayment struct for monitored TXID: ${txid}`);
    }
  });
});

describe("Data Report Submission", () => {
  it("Submits multiple data reports for different TXIDs with consensus and dissent", async () => {
    const seedPreamble = "pastel_tx_status_report";

    // Transfer SOL to each contributor
    const transferAmountSOL = 1.0;
    for (const contributor of contributors) {
      const transferTransaction = new anchor.web3.Transaction().add(
        anchor.web3.SystemProgram.transfer({
          fromPubkey: admin.publicKey,
          toPubkey: contributor.publicKey,
          lamports: transferAmountSOL * anchor.web3.LAMPORTS_PER_SOL,
        })
      );

      await provider.sendAndConfirm(transferTransaction);
      console.log(
        `Transferred ${transferAmountSOL} SOL to contributor account with address ${contributor.publicKey.toBase58()}`
      );
    }

    // Find the PDA for the ContributorDataAccount
    const [contributorDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("contributor_data")],
        program.programId
      );

    // Find the PDA for the TxidSubmissionCountsAccount
    const [txidSubmissionCountsAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("txid_submission_counts")],
        program.programId
      );

    // Find the PDA for the AggregatedConsensusDataAccount
    const [aggregatedConsensusDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("aggregated_consensus_data")],
        program.programId
      );

    // Fetch monitored TXIDs from the updated state
    const state = await program.account.oracleContractState.fetch(
      oracleContractState.publicKey
    );
    const monitoredTxids = state.monitoredTxids;

    // Loop through each monitored TXID
    for (const txid of monitoredTxids) {
      // Generate a random file hash for this TXID
      const randomFileHash = [...Array(6)]
        .map(() => Math.floor(Math.random() * 16).toString(16))
        .join("");
      console.log(
        `Random file hash (first 6 characters) for TXID ${txid} generated as:`,
        randomFileHash
      );

      for (let i = 0; i < contributors.length; i++) {
        const contributor = contributors[i];
        const rewardAddress = contributor.publicKey;

        // Determine the probability of submitting an incorrect report
        const errorProbability =
          i < BAD_CONTRIBUTOR_INDEX
            ? 0
            : (i - BAD_CONTRIBUTOR_INDEX + 1) /
              (contributors.length - BAD_CONTRIBUTOR_INDEX);
        const isIncorrect = Math.random() < errorProbability;

        // Randomize the status value for each report
        const txidStatusValue = isIncorrect
          ? TxidStatusEnum.Invalid
          : TxidStatusEnum.MinedActivated;
        const pastelTicketTypeValue = PastelTicketTypeEnum.Nft;

        console.log(
          `Status value for TXID ${txid} by contributor ${contributor.publicKey.toBase58()} is '${txidStatusValue}'; ticket type value is '${pastelTicketTypeValue}'`
        );

        const preimageString = seedPreamble + txid + rewardAddress.toBase58();

        const preimageBytes = Buffer.from(preimageString, "utf8");

        const seedHash = crypto
          .createHash("sha256")
          .update(preimageBytes)
          .digest();

        const [reportAccountPDA] = web3.PublicKey.findProgramAddressSync(
          [seedHash],
          program.programId
        );

        // Derive the PDA for TempTxStatusReportAccount
        const [tempReportAccountPDA] = web3.PublicKey.findProgramAddressSync(
          [Buffer.from("temp_tx_status_report")],
          program.programId
        );

        // Create and send the transaction
        try {
          const transaction = new Transaction();
          transaction.add(
            ComputeBudgetProgram.setComputeUnitLimit({ units: 1400000 })
          );

          const submitDataReportInstruction = await program.methods
            .submitDataReport(
              txid,
              txidStatusValue,
              pastelTicketTypeValue,
              randomFileHash,
              contributor.publicKey
            )
            .accounts({
              reportAccount: reportAccountPDA,
              tempReportAccount: tempReportAccountPDA,
              contributorDataAccount: contributorDataAccountPDA,
              txidSubmissionCountsAccount: txidSubmissionCountsAccountPDA,
              aggregatedConsensusDataAccount: aggregatedConsensusDataAccountPDA,
              oracleContractState: oracleContractState.publicKey,
              user: contributor.publicKey,
              systemProgram: web3.SystemProgram.programId,
            })
            .signers([contributor])
            .instruction();

          transaction.add(submitDataReportInstruction);

          // Attempt to submit the data report
          try {
            await provider.sendAndConfirm(transaction, [contributor]);
            console.log(`Data report for TXID ${txid} submitted by contributor ${contributor.publicKey.toBase58()}`);
        } catch (error) {
            const errorString = error.toString();
        
            // Extracting the error code
            const match = errorString.match(/custom program error: 0x(\w+)/);
            if (match) {
                const errorCode = parseInt(match[1], 16);
                const errorName = ErrorCodeMap[errorCode];
        
                if (errorName === 'ContributorBanned' && i >= BAD_CONTRIBUTOR_INDEX) {
                    console.log(`Expected 'ContributorBanned' error for contributor ${contributor.publicKey.toBase58()}: ${errorString}`);
                } else if (errorName === 'EnoughReportsSubmittedForTxid' && i >= MIN_NUMBER_OF_ORACLES) {
                    console.log(`Expected 'EnoughReportsSubmittedForTxid' error for contributor ${contributor.publicKey.toBase58()}: ${errorString}`);
                } else {
                    console.error(`Unexpected error: ${errorName || 'Unknown error'} - ${errorString}`);
                    // Decide if you want to throw the error or continue
                }
            } else {
                console.error(`Error parsing error code: ${errorString}`);
                // Decide on handling unparsed errors
            }
        }
        
        } catch (error) {
          console.error(
            `Error submitting report for TXID ${txid} by contributor ${contributor.publicKey.toBase58()}:`,
            error
          );
          throw error;
        }


      }

      // Fetch the consensus data from the AggregatedConsensusDataAccount PDA
      const aggregatedConsensusDataAccount =
        await program.account.aggregatedConsensusDataAccount.fetch(
          aggregatedConsensusDataAccountPDA
        );
      const consensusData = aggregatedConsensusDataAccount.consensusData.find(
        (data) => data.txid === txid
      );

      if (consensusData) {
        const consensusStatusIndex = consensusData.statusWeights.indexOf(
          Math.max(...consensusData.statusWeights)
        );
        const consensusStatus = [
          "Invalid",
          "PendingMining",
          "MinedPendingActivation",
          "MinedActivated",
        ][consensusStatusIndex];
        console.log(`Consensus Status for TXID ${txid}:`, consensusStatus);
      } else {
        console.log(`No consensus data found for TXID ${txid}`);
      }
    }

    // Loop through each monitored TXID for validation
    for (const txid of monitoredTxids) {

      // Fetch the updated state after all submissions for this TXID
      const updatedState = await program.account.oracleContractState.fetch(
        oracleContractState.publicKey
      );
      // console.log(`Updated Oracle Contract State for TXID ${txid}:`, updatedState);

      // Check if the txid is in the monitored list
      assert(
        updatedState.monitoredTxids.includes(txid),
        `TXID ${txid} should be in the monitored list`
      );

      // Fetch the consensus data from the AggregatedConsensusDataAccount PDA
      const aggregatedConsensusDataAccount =
      await program.account.aggregatedConsensusDataAccount.fetch(
          aggregatedConsensusDataAccountPDA
      );
      const consensusData = aggregatedConsensusDataAccount.consensusData.find(
          (data) => data.txid === txid
      );
      
      assert(
        consensusData !== undefined,
        `Consensus data should be present for the TXID ${txid}`
      );

      // Assuming the consensus is based on the weighted majority rule
      const consensusStatusIndex = consensusData.statusWeights.indexOf(
        Math.max(...consensusData.statusWeights)
      );
      const consensusStatus = [
        "Invalid",
        "PendingMining",
        "MinedPendingActivation",
        "MinedActivated",
      ][consensusStatusIndex];
      console.log(`Consensus Status for TXID ${txid}:`, consensusStatus);

      // Check if the majority consensus is achieved for 'MinedActivated'
      assert(
        consensusStatus === "MinedActivated",
        `Majority consensus for TXID ${txid} should be 'MinedActivated'`
      );

      // Check for the hash with the highest weight
      const consensusHash = consensusData.hashWeights.reduce(
        (max, h) => (max.weight > h.weight ? max : h),
        { hash: "", weight: -1 }
      ).hash;
      console.log(`Consensus Hash for TXID ${txid}:`, consensusHash);

      trackedTxids.push(txid);

      // Add further checks if needed based on the contract's consensus logic
      console.log(
        `Data report submission verification successful for the TXID: ${txid}`
      );
    }

    console.log(
      `Data report submission verification successful for all monitored TXIDs`
    );
    console.log(`______________________________________________________`);
    console.log(`Now checking the state of each contributor:`);
    //Loop through contributors to show their state:
    const contributorData = await program.account.contributorDataAccount.fetch(
      contributorDataAccountPDA
    );

    for (const contributor of contributors) {
      const currentContributorData = contributorData.contributors.find((c) =>
        c.rewardAddress.equals(contributor.publicKey)
      );

      // Check if the contributor data exists
      if (!currentContributorData) {
        throw new Error(
          `Contributor data not found for address ${contributor.publicKey.toBase58()}`
        );
      }

      // Log all fields for each contributor
      console.log(`Contributor: ${contributor.publicKey.toBase58()}`);
      console.log(
        `Registration Entrance Fee Transaction Signature: ${currentContributorData.registrationEntranceFeeTransactionSignature}`
      );
      console.log(
        `Compliance Score: ${currentContributorData.complianceScore}`
      );
      console.log(
        `Last Active Timestamp: ${currentContributorData.lastActiveTimestamp}`
      );
      console.log(
        `Total Reports Submitted: ${currentContributorData.totalReportsSubmitted}`
      );
      console.log(
        `Accurate Reports Count: ${currentContributorData.accurateReportsCount}`
      );
      console.log(`Current Streak: ${currentContributorData.currentStreak}`);
      console.log(
        `Reliability Score: ${currentContributorData.reliabilityScore}`
      );
      console.log(
        `Consensus Failures: ${currentContributorData.consensusFailures}`
      );
      console.log(`Ban Expiry: ${currentContributorData.banExpiry}`);
      console.log(
        `Is Eligible for Rewards: ${currentContributorData.isEligibleForRewards}`
      );
      console.log(
        `Is Recently Active: ${currentContributorData.isRecentlyActive}`
      );
      console.log(`Is Reliable: ${currentContributorData.isReliable}`);
      console.log(`______________________________________________________`);
    }
  });
});

describe("Data Cleanup Verification", () => {
  it("Verifies that data is cleaned up post-consensus", async () => {
    const txidsToCheck = trackedTxids;

    const [tempReportAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("temp_tx_status_report")],
      program.programId
    );

    const tempReportAccountData = await program.account.tempTxStatusReportAccount.fetch(tempReportAccountPDA);

    txidsToCheck.forEach(txid => {
      const isTxidPresentInTempReport = tempReportAccountData.reports.some(report => {
        const commonDataIndex = report.commonDataRef.toNumber(); // Convert BN to number
        const commonData = tempReportAccountData.commonReports[commonDataIndex]; // Use converted index
        return commonData.txid === txid;
      });
      assert.isFalse(
        isTxidPresentInTempReport,
        `TXID ${txid} should be cleaned up from TempTxStatusReportAccount`
      );

    });

    console.log("Data cleanup post-consensus verification completed successfully.");
  });
});


describe("Payment Processing by Bridge Contract", () => {
  it("Processes payments for monitored TXIDs", async () => {
    const COST_IN_LAMPORTS_OF_ADDING_PASTEL_TXID_FOR_MONITORING = new BN(
      COST_IN_SOL_OF_ADDING_PASTEL_TXID_FOR_MONITORING * web3.LAMPORTS_PER_SOL
    );
    // Derive the FeeReceivingContractAccount PDA
    const [feeReceivingContractAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("fee_receiving_contract")],
        program.programId
      );

    // Fetch monitored TXIDs from the updated state
    const state = await program.account.oracleContractState.fetch(
      oracleContractState.publicKey
    );
    const monitoredTxids = state.monitoredTxids;

    for (const txid of monitoredTxids) {
      // Transfer the payment for the TXID from the admin account to the fee-receiving contract
      const transferTransaction = new anchor.web3.Transaction().add(
        anchor.web3.SystemProgram.transfer({
          fromPubkey: admin.publicKey,
          toPubkey: feeReceivingContractAccountPDA,
          lamports:
            COST_IN_LAMPORTS_OF_ADDING_PASTEL_TXID_FOR_MONITORING.toNumber(),
        })
      );

      await provider.sendAndConfirm(transferTransaction);
      console.log(
        `Transferred payment for TXID ${txid} to fee-receiving contract`
      );

      // Manually create the seed for pendingPaymentAccountPDA
      const preimageString =
        "pending_payment" + txid + admin.publicKey.toBase58();
      const preimageBytes = Buffer.from(preimageString, "utf8");
      const seedHash = crypto
        .createHash("sha256")
        .update(preimageBytes)
        .digest();
      const [pendingPaymentAccountPDA] = web3.PublicKey.findProgramAddressSync(
        [seedHash],
        program.programId
      );

      // Process the payment in the oracle contract
      await program.methods
        .processPayment(
          txid,
          COST_IN_LAMPORTS_OF_ADDING_PASTEL_TXID_FOR_MONITORING
        )
        .accounts({
          sourceAccount: admin.publicKey,
          oracleContractState: oracleContractState.publicKey,
          pendingPaymentAccount: pendingPaymentAccountPDA,
          systemProgram: web3.SystemProgram.programId,
        })
        .rpc();

      // Fetch the updated PendingPayment struct to verify the payment status
      const pendingPaymentData =
        await program.account.pendingPaymentAccount.fetch(
          pendingPaymentAccountPDA
        );

      // Convert paymentStatus to JSON and compare the stringified version
      const paymentStatusJson = JSON.stringify(
        pendingPaymentData.pendingPayment.paymentStatus
      );
      assert.strictEqual(
        paymentStatusJson,
        JSON.stringify({ received: {} }),
        `The payment status for TXID: ${txid} should be 'Received'`
      );

      console.log(`Payment processed for TXID ${txid}`);
    }
  });
});


describe("Eligibility for Rewards", () => {
  it("should check if contributors meet reward eligibility criteria", async () => {
    // Fetch the ContributorDataAccount
    const [contributorDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("contributor_data")],
        program.programId
      );

    const contributorData = await program.account.contributorDataAccount.fetch(
      contributorDataAccountPDA
    );

    // Loop through each contributor and check their eligibility
    for (const contributor of contributors) {
      const currentContributorData = contributorData.contributors.find((c) =>
        c.rewardAddress.equals(contributor.publicKey)
      );

      // Check if the contributor data exists
      if (!currentContributorData) {
        throw new Error(
          `Contributor data not found for address ${contributor.publicKey.toBase58()}`
        );
      }

      // Log all fields for each contributor
      console.log(`Contributor: ${contributor.publicKey.toBase58()}`);
      console.log(
        `Registration Entrance Fee Transaction Signature: ${currentContributorData.registrationEntranceFeeTransactionSignature}`
      );
      console.log(
        `Compliance Score: ${currentContributorData.complianceScore}`
      );
      console.log(
        `Last Active Timestamp: ${currentContributorData.lastActiveTimestamp}`
      );
      console.log(
        `Total Reports Submitted: ${currentContributorData.totalReportsSubmitted}`
      );
      console.log(
        `Accurate Reports Count: ${currentContributorData.accurateReportsCount}`
      );
      console.log(`Current Streak: ${currentContributorData.currentStreak}`);
      console.log(
        `Reliability Score: ${currentContributorData.reliabilityScore}`
      );
      console.log(
        `Consensus Failures: ${currentContributorData.consensusFailures}`
      );
      console.log(`Ban Expiry: ${currentContributorData.banExpiry}`);
      console.log(
        `Is Eligible for Rewards: ${currentContributorData.isEligibleForRewards}`
      );
      console.log(
        `Is Recently Active: ${currentContributorData.isRecentlyActive}`
      );
      console.log(`Is Reliable: ${currentContributorData.isReliable}`);
      console.log(`______________________________________________________`);

      // Define your eligibility criteria based on your contract logic
      const isEligibleForRewards =
        currentContributorData.totalReportsSubmitted >= MIN_REPORTS_FOR_REWARD &&
        currentContributorData.reliabilityScore >= MIN_RELIABILITY_SCORE_FOR_REWARD &&
        currentContributorData.complianceScore >= MIN_COMPLIANCE_SCORE_FOR_REWARD;

      assert(
        currentContributorData.isEligibleForRewards === isEligibleForRewards,
        `Eligibility for rewards for contributor with address ${contributor.publicKey.toBase58()} should be correctly determined`
      );

    }
  });
});

describe('Reward Distribution for Eligible Contributor', () => {
  it('should distribute rewards correctly from the reward pool', async () => {
    // Choose an eligible contributor
    const eligibleContributor = contributors[0]; // Assuming the first contributor is eligible

    // Find the PDA for the RewardPoolAccount
    const [rewardPoolAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("reward_pool")],
      program.programId
    );

    const [contributorDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("contributor_data")],
        program.programId
      );

    // Get initial balance of the reward pool
    const initialRewardPoolBalance = await provider.connection.getBalance(rewardPoolAccountPDA);

    // Get initial balance of the eligible contributor
    const initialContributorBalance = await provider.connection.getBalance(eligibleContributor.publicKey);

    // Request reward for the eligible contributor
    await program.methods.requestReward(eligibleContributor.publicKey)
      .accounts({
        rewardPoolAccount: rewardPoolAccountPDA,
        oracleContractState: oracleContractState.publicKey,
        contributorDataAccount: contributorDataAccountPDA
      })
      .rpc();

    // Wait for the transaction to be confirmed
    await new Promise(resolve => setTimeout(resolve, 1000));  // Add a delay

    // Get updated balance of the reward pool
    const updatedRewardPoolBalance = await provider.connection.getBalance(rewardPoolAccountPDA);

    // Check if the balance is deducted correctly
    const expectedBalanceAfterReward = initialRewardPoolBalance - BASE_REWARD_AMOUNT_IN_LAMPORTS;
    assert.equal(updatedRewardPoolBalance, expectedBalanceAfterReward, 'Reward pool balance should be deducted by the reward amount');

    // Get updated balance of the eligible contributor
    const updatedContributorBalance = await provider.connection.getBalance(eligibleContributor.publicKey);

    // Define a tolerance for transaction fees
    const feeTolerance = 5000; // Example value, adjust as needed

    // Check if the reward is transferred to the contributor's address within the tolerance range
    const minExpectedBalance = initialContributorBalance + BASE_REWARD_AMOUNT_IN_LAMPORTS - feeTolerance;
    const maxExpectedBalance = initialContributorBalance + BASE_REWARD_AMOUNT_IN_LAMPORTS + feeTolerance;
    assert.isTrue(
      updatedContributorBalance >= minExpectedBalance && updatedContributorBalance <= maxExpectedBalance,
      'Eligible contributor should receive the reward amount within the specified tolerance range'
    );
  });
});

describe('Request Reward for Ineligible Contributor', () => {
  it('should not allow reward requests from ineligible contributors', async () => {
    // Choose an ineligible contributor
    const ineligibleContributor = contributors[contributors.length - 1]; // Assuming the last contributor is ineligible

    // Find the PDA for the RewardPoolAccount
    const [rewardPoolAccountPDA] = await web3.PublicKey.findProgramAddressSync(
      [Buffer.from("reward_pool")],
      program.programId
    );

    const [contributorDataAccountPDA] =
      await web3.PublicKey.findProgramAddressSync(
        [Buffer.from("contributor_data")],
        program.programId
      );

    try {
      // Attempt to request reward
      await program.methods.requestReward(ineligibleContributor.publicKey)
        .accounts({
          rewardPoolAccount: rewardPoolAccountPDA,
          oracleContractState: oracleContractState.publicKey,
          contributorDataAccount: contributorDataAccountPDA
        })
        .rpc();

      throw new Error('Reward request should have failed for ineligible contributor');
    } catch (error) {
      const errorString = error.toString();

      // Extracting the error code
      const match = errorString.match(/custom program error: 0x(\w+)/);
      if (match) {
          const errorCode = parseInt(match[1], 16);
          const errorName = ErrorCodeMap[errorCode];

          // Check for the specific error thrown by your program
          assert.equal(errorName, 'NotEligibleForReward', 'Should throw NotEligibleForReward error');
      } else {
          console.error(`Error parsing error code: ${errorString}`);
          // Decide on handling unparsed errors
      }
    }
  });
});


// Remaining tests to add:

// Reallocation of Oracle State:

//     Test the functionality that allows dynamic reallocation of space for various program accounts.
//     Simulate conditions where accounts reach their initial capacity and require reallocation to accommodate additional data.
//     Ensure that during reallocation, no data is lost or corrupted, and the program maintains its integrity and performance.

// Withdrawal of Funds:

//     Verify that the withdrawal functionality works correctly, allowing the admin to securely withdraw funds from the reward pool and fee-receiving contract.
//     Test different withdrawal scenarios, including partial withdrawals and attempts to withdraw amounts exceeding the account balances.
//     Implement security tests to ensure that only the authorized admin can perform withdrawal actions, protecting against unauthorized access or exploitation.
//     Validate the correct transfer and balance adjustments post-withdrawal, ensuring the program's accounting is accurate.

// Reinitialization Prevention:
// This testing ensures that program-derived accounts (PDAs) are not susceptible to unintended reinitialization, which could reset or corrupt the stored state.
// Implement tests to attempt reinitialization of various accounts, particularly PDAs like OracleContractState, RewardPoolAccount, FeeReceivingContractAccount, etc., and verify that these attempts fail.
// Such tests safeguard against vulnerabilities where an attacker or erroneous operation might reset critical state data, impacting the program's integrity.

// Contract Upgrade Path:
// If your program design includes provisions for future upgrades (a common practice in blockchain applications for scalability, feature addition, or bug fixes), it's crucial to test the upgrade process.
// Simulate contract upgrades to ensure that the new version of the program maintains continuity with the existing state. This includes verifying that data in various accounts is preserved and remains consistent post-upgrade.
// This aspect of testing is vital for ensuring that future upgrades do not disrupt the ongoing operations or data integrity of your program.

// Resource Utilization and Cost Analysis:
// Solana programs consume resources like compute units, and transactions have associated costs. It's important to analyze and test these aspects to optimize performance and cost-efficiency.
// Perform tests to measure the resource utilization of various functions, especially those that are computationally intensive or called frequently. This helps identify potential bottlenecks or inefficiencies.
// Analyze transaction costs to ensure that they align with expectations and are manageable within the economic model of your application. This is particularly important for functions that users will call frequently.