import * as anchor from '@coral-xyz/anchor';
import { SolanaPastelOracleProgram, IDL } from './target/types/solana_pastel_bridge_program';
import { web3 } from '@coral-xyz/anchor';

async function main() {
    // Set the provider URL directly
    const providerUrl = "http://127.0.0.1:8899"; // Or use another cluster URL
    const connection = new anchor.web3.Connection(providerUrl, "processed");

    const wallet = new anchor.Wallet(anchor.web3.Keypair.generate());
    const provider = new anchor.AnchorProvider(connection, wallet, { preflightCommitment: "processed" });
    anchor.setProvider(provider);

    const programID = new anchor.web3.PublicKey("AfP1c4sFcY1FeiGjQEtyxCim8BRnw22okNbKAsH2sBsB");
    const program = new anchor.Program<SolanaPastelOracleProgram>(IDL, programID, provider);

    // Keypairs for necessary accounts
    const admin = web3.Keypair.generate();
    const oracleContractState = web3.Keypair.generate();
    const rewardPoolAccount = web3.Keypair.generate();
    const feeReceivingContractAccount = web3.Keypair.generate();
    const newContributor = web3.Keypair.generate();

    // Initialize the Oracle Contract State
    await program.methods.initialize(admin.publicKey)
        .accounts({
            oracleContractState: oracleContractState.publicKey,
            user: admin.publicKey,
            rewardPoolAccount: rewardPoolAccount.publicKey,
            feeReceivingContractAccount: feeReceivingContractAccount.publicKey,
            systemProgram: web3.SystemProgram.programId,
        })
        .signers([admin, oracleContractState, rewardPoolAccount, feeReceivingContractAccount])
        .rpc();

    // Register a new contributor
    await program.methods.registerNewDataContributor()
        .accounts({
            oracleContractState: oracleContractState.publicKey,
            contributorAccount: newContributor.publicKey,
            rewardPoolAccount: rewardPoolAccount.publicKey,
            feeReceivingContractAccount: feeReceivingContractAccount.publicKey,
        })
        .signers([newContributor])
        .rpc();

    // Fetch Contributor data
    const contributorData = await program.contributor.fetch(newContributor.publicKey);

    console.log("Contributor data:", contributorData);
}

main().then(() => process.exit(0)).catch(error => {
    console.error(error);
    process.exit(1);
});
