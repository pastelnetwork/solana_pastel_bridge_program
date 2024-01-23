# Solana-Pastel Oracle Program

## Overview
This program serves as an oracle service, creating a bridge between the Solana blockchain and the Pastel Network. It focuses on verifying and monitoring Pastel transaction IDs (TXIDs) and related blockchain tickets, as well as the files associated with these tickets.

## Functionality
- **Transaction Monitoring**: Tracks Pastel TXIDs, updating their status (e.g., pending, mined) within the Solana blockchain.
- **Data Compliance and Reward System**: Implements a system for contributors to submit reports on Pastel TXID statuses, with rewards based on report accuracy and reliability.
- **Consensus Algorithm**: Uses a consensus mechanism to ascertain the most accurate status of Pastel TXIDs, based on multiple oracle reports.
- **Ban Mechanism**: Features temporary and permanent bans for contributors based on non-consensus report submissions.

## Core Components
- **OracleContractState**: Maintains the central state of the oracle, including contributor data, consensus data, and TXID submission counts.
- **Contributor Management**: Registers new data contributors and manages their compliance and reliability scores. Contributors must meet certain criteria to be eligible for rewards.
- **Report Submission and Processing**: Manages the submission of Pastel TXID status reports by contributors and processes these reports to update consensus data and contributor scores.
- **Consensus Calculation**: Determines the consensus on TXID status based on aggregated data from multiple reports.
- **Reward and Penalty Mechanisms**: Manages rewards distribution to contributors and applies ban penalties in case of report inconsistencies.
- **Fee and Payment Management**: Handles the addition of new Pastel TXIDs for monitoring, including payment processing for such additions.

## Key Constants and Variables
- **Registration and Reward Fees**: Specifies the fees for contributor registration and the base reward amount for report submissions.
- **Ban Thresholds**: Defines the thresholds for temporary and permanent bans based on report submission accuracy.
- **Consensus Parameters**: Establishes the minimum requirements for consensus calculation among oracle reports.

## Security and Validation
- Ensures data integrity through rigorous checks at every step, from contributor registration to report submission and consensus calculation.
- Implements a robust validation mechanism to prevent incorrect or fraudulent data submissions.

## Setup Instructions

### Install Solana Testnet on Ubuntu

#### Install Rustup
```bash
curl https://sh.rustup.rs -sSf | sh
rustup default nightly  
rustup update nightly   
rustc --version 
```

#### Install Solana
```bash
sudo apt update && sudo apt upgrade -y && sudo apt autoremove -y  
sudo apt install libssl-dev libudev-dev pkg-config zlib1g-dev llvm clang make -y         
sh -c "$(curl -sSfL https://release.solana.com/v1.17.13/install)"      
export PATH="/home/ubuntu/.local/share/solana/install/active_release/bin:$PATH" 
source ~/.zshrc   # If you use Zsh
solana --version  
```

### Setup Anchor

```bash
sudo apt-get update && sudo apt-get upgrade && sudo apt-get install -y pkg-config build-essential libudev-dev
cargo install --git https://github.com/coral-xyz/anchor avm --locked --force
avm install latest
avm use latest
anchor --version
```

### Get Code and Test

```bash
git clone https://github.com/pastelnetwork/solana_pastel_oracle_program.git
cd solana_pastel_oracle_program
anchor test
```

These steps will set up the necessary environment for running the Solana-Pastel Oracle Program, including the installation of Rust, Solana, and Anchor, as well as cloning the repository and running tests to ensure everything is correctly set up.


## Testing Code

The testing code for the Solana-Pastel Oracle Program, written in TypeScript using the Anchor framework, performs comprehensive tests to validate the functionality of the program. It uses Mocha for structuring tests and Chai for assertions. Here's an overview of its components and functionalities:

1. **Setup and Configuration**: 
    - Initializes Anchor with the Solana testnet and sets various configurations.
    - Defines constants and variables such as program ID, contributor details, TXID counts, and error codes.

2. **Initialization Tests**: 
    - Tests the initialization and expansion of the oracle contract state.
    - Uses PDAs (Program-Derived Addresses) for various accounts like reward pool, fee-receiving contract, and contributor data.
    - Confirms the contract state's initialization and checks the reallocation of the oracle state.

3. **Set Bridge Contract Tests**: 
    - Ensures the bridge contract address is correctly set to the admin address.

4. **Contributor Registration Tests**: 
    - Registers new data contributors and validates their registration.
    - Transfers the registration fee to the required accounts and confirms the contributor's registration in the system.

5. **TXID Monitoring Tests**: 
    - Adds multiple TXIDs for monitoring and checks if they are correctly monitored.
    - Verifies the creation of corresponding `PendingPayment` structs for each TXID.

6. **Data Report Submission Tests**: 
    - Submits data reports for different TXIDs to simulate both consensus and dissent among contributors.
    - Determines the status of TXIDs (e.g., MinedActivated, Invalid) and submits reports accordingly.
    - Validates the consensus on TXID status after the reports are submitted.

7. **Data Cleanup Verification Tests**: 
    - Ensures that data related to TXIDs is properly cleaned up from the `TempTxStatusReportAccount` after consensus is reached.

8. **Payment Processing Tests**: 
    - Simulates the processing of payments for monitored TXIDs by the bridge contract.
    - Transfers payments and confirms that the payments are correctly recorded in the system.

9. **Eligibility for Rewards Tests**: 
    - Checks if contributors meet the eligibility criteria for rewards based on compliance, reliability scores, and report submissions.

10. **Reward Distribution Tests**: 
    - Tests the distribution of rewards from the reward pool to eligible contributors.
    - Verifies that ineligible contributors cannot request rewards.

Throughout these tests, the code interacts with various accounts and PDAs, simulating real-world scenarios of contributor activities and interactions with the oracle contract. The tests cover various aspects of the program, including state initialization, contributor management, transaction monitoring, consensus mechanism, and reward distribution, ensuring the program functions as intended.

## Consensus Process and Contributor Scoring System

### Consensus Process

The consensus process in the Solana-Pastel Oracle Program is designed to determine the most reliable status of a Pastel transaction ID (TXID) based on reports from multiple contributors. Here's how it works:

1. **Aggregating Data**: The `get_aggregated_data` function retrieves the aggregated consensus data for a given TXID, which includes the weighted status and hash values reported by contributors.

2. **Computing Consensus**: The `compute_consensus` function calculates the consensus status and hash for the TXID. It identifies the status and hash with the highest accumulated weight as the consensus outcome.

3. **Consensus Decision**: The consensus status represents the most agreed-upon state of the TXID, and the consensus hash represents the most agreed-upon hash of the corresponding file.

### Rationale Behind the Consensus Design

- **Robustness**: Using a weighted approach where each contributor's report influences the consensus based on their reliability and compliance score ensures robustness against inaccurate or malicious reports.
- **Flexibility**: The mechanism can adapt to different scenarios, like new information or changing network conditions, by recalculating the consensus with each new report.

### Contributor Scoring System

The scoring system for contributors is a crucial aspect of the program, affecting their influence in the consensus process and their eligibility for rewards.

#### Score Adjustment Mechanisms

1. **Updating Scores (`update_scores`)**: Contributors’ scores are dynamically adjusted based on their report accuracy. Accurate reports increase their compliance and reliability scores, while inaccurate reports lead to a decrease.

2. **Applying Bans (`apply_bans`)**: Contributors with a pattern of inaccurate reporting are subject to temporary or permanent bans, reducing the risk of bad actors influencing the consensus process.

3. **Time-Based Decay**: The system implements a decay factor on scores over time, encouraging continuous and consistent participation.

#### Rationale Behind Scoring Components

- **Accuracy and Streaks**: Rewarding accuracy and consistency (streaks) incentivizes contributors to provide reliable data. The scaling bonus for consecutive accurate reports encourages sustained high-quality participation.
  
- **Time Weight**: The time since the last active contribution is factored into score adjustments. This ensures that active contributors have a more significant impact on the consensus and rewards.

- **Decay Mechanism**: To maintain a dynamic and responsive system, scores decay over time. This prevents long-inactive contributors from retaining high influence and encourages regular participation.

- **Reliability Integration**: A contributor’s overall reliability (ratio of accurate reports to total reports) is integrated into their compliance score, ensuring that consistently reliable contributors have more influence.

#### Purpose of the Scoring System

- **Quality Control**: Ensures that the data influencing the consensus process is of high quality and from reliable sources.
- **Incentive Alignment**: Aligns contributors' incentives with the network's goal of accurate and reliable data reporting.
- **Adaptability**: Allows the system to adapt to changing participant behaviors and network conditions by recalibrating contributor influence dynamically.
- **Security**: Protects against manipulation or attacks by devaluing the influence of malicious or consistently inaccurate contributors.