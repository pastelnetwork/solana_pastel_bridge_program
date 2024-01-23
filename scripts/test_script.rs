use solana_program::{
    pubkey::Pubkey,

};

use anchor_lang::solana_program::hash::{hash, Hash};


fn main() {
    let seed_preamble = "pastel_tx_status_report";
    let txid = "9930511c526808e6849a25cb0eb6513f729c2a71ec51fbca084d7c7e4a8dea2f";
    let reward_address = Pubkey::new_unique(); // For demonstration, using a unique pubkey

    let seed_hash = create_seed(seed_preamble, txid, &reward_address);
    println!("String Seed Bytes: {:?}", seed_preamble.as_bytes());
    println!("TXID Seed Bytes: {:?}", txid.as_bytes());
    println!("Public Key Seed Bytes: {:?}", reward_address.as_ref());
    
    println!("Seed Preamble: {}", seed_preamble);
    println!("Transaction ID: {}", txid);
    println!("Reward Address: {}", reward_address);
    println!("Seed Hash: {:?}", seed_hash);
    println!("Generated PDA: {:?}", Pubkey::create_program_address(&[&seed_hash.as_ref()], &Pubkey::new_unique())); // Unique program ID for demonstration

}

fn create_seed(seed_preamble: &str, txid: &str, reward_address: &Pubkey) -> Hash {
    let mut preimage = Vec::new();
    preimage.extend_from_slice(seed_preamble.as_bytes());
    preimage.extend_from_slice(txid.as_bytes());
    preimage.extend_from_slice(reward_address.as_ref());
    hash(&preimage)
}
