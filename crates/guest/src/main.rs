#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::{Digest, Sha256};
use tiny_mlp::{forward, weights_to_bytes, WEIGHTS_LEN};

pub fn main() {
    // Read private inputs via zero-copy rkyv deserialization.
    // rkyv costs far fewer cycles than bincode (the default for io::read).
    // The prover builds stdin with write_slice + rkyv::to_bytes — must match.
    let input_bytes: Vec<u8> = sp1_zkvm::io::read_vec();
    let weights_bytes: Vec<u8> = sp1_zkvm::io::read_vec();
    let model_id: [u8; 32] = sp1_zkvm::io::read(); // public commitment

    let input: Vec<f32> =
        unsafe { rkyv::from_bytes_unchecked(&input_bytes).expect("input deserialize failed") };
    let weights: Vec<f32> =
        unsafe { rkyv::from_bytes_unchecked(&weights_bytes).expect("weights deserialize failed") };
    assert_eq!(weights.len(), WEIGHTS_LEN);
    assert_eq!(input.len(), 4);

    // Verify weights match committed modelId — prevents prover from swapping weights.
    // sha2 patch routes this through SP1's SHA-256 precompile automatically.
    println!("cycle-tracker-start: weight-hash");
    let computed: [u8; 32] = Sha256::digest(&weights_to_bytes(&weights)).into();
    assert_eq!(
        computed, model_id,
        "weights do not match committed model_id"
    );
    println!("cycle-tracker-end: weight-hash");

    println!("cycle-tracker-start: inference");
    let output = forward(&weights, &input);
    println!("cycle-tracker-end: inference");

    // Commit public values visible in the proof and on-chain
    let input_le: Vec<u8> = input.iter().flat_map(|f| f.to_le_bytes()).collect();
    let output_le: Vec<u8> = output.iter().flat_map(|f| f.to_le_bytes()).collect();

    let input_hash: [u8; 32] = Sha256::digest(&input_le).into();
    let output_hash: [u8; 32] = Sha256::digest(&output_le).into();

    sp1_zkvm::io::commit(&model_id);
    sp1_zkvm::io::commit(&input_hash);
    sp1_zkvm::io::commit(&output_hash);
}
