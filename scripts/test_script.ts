import * as crypto from 'crypto';
import { PublicKey } from '@solana/web3.js';
import bs58 from 'bs58';

async function generateTestHash() {
    const testString = "this is a test";

    // Convert to bytes using UTF-8 encoding
    const testBytes = Buffer.from(testString, 'utf8');

    // Compute SHA256 hash
    const sha256Hash = crypto.createHash('sha256').update(testBytes).digest();

    // Convert SHA256 hash to Base58
    const base58Hash = bs58.encode(sha256Hash);

    console.log("Test String:", testString);
    console.log("Test Hash (Base58):", base58Hash);
}

async function generateSeedHash() {
    const seedPreamble = "pastel_tx_status_report";
    const txid = "9930511c526808e6849a25cb0eb6513f729c2a71ec51fbca084d7c7e4a8dea2f";
    const rewardAddress = new PublicKey("1111111QLbz7JHiBTspS962RLKV8GndWFwiEaqKM");

    // Concatenate strings
    const preimageString = seedPreamble + txid + rewardAddress.toString();

    // Convert to bytes using UTF-8 encoding
    const preimageBytes = Buffer.from(preimageString, 'utf8');

    // Compute hash
    const seedHash = crypto.createHash('sha256').update(preimageBytes).digest();

    console.log("Preimage String:", preimageString);
    console.log("Seed Hash:", seedHash.toString('hex'));

    // Generate PDA
    const programId = new PublicKey("11111111111111111111111111111111");
    const [pda, _] = await PublicKey.findProgramAddress([seedHash], programId);

    console.log("Generated PDA:", pda.toString());
}

generateTestHash();

generateSeedHash();
