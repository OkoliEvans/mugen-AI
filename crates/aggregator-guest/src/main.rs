// crates/aggregator-guest/src/main.rs
//! Aggregator guest program — verifies N compressed SP1 inference proofs
//! inside the zkVM and commits a merkle root of all output_hashes.
//!
//! Stdin layout (must match aggregator/src/lib.rs exactly):
//!   write::<Vec<[u32; 8]>>(&vkeys)          — one vkey per proof
//!   write::<Vec<Vec<u8>>>(&public_values)    — one public_values per proof
//!   write_proof(proof, vk) x N              — proofs fed automatically by prover
//!
//! Public values committed:
//!   [u8; 32] merkle_root  — sha256 tree over all output_hashes
//!   u32      batch_size   — number of proofs aggregated

#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::{Digest, Sha256};

pub fn main() {
    // Read vkeys — one [u32; 8] per proof
    let vkeys = sp1_zkvm::io::read::<Vec<[u32; 8]>>();

    // Read public values — one Vec<u8> per proof
    // Each is 112 bytes: model_id[32] + input_hash[32] + output_hash[32] + output[16]
    let all_public_values = sp1_zkvm::io::read::<Vec<Vec<u8>>>();

    assert_eq!(
        vkeys.len(),
        all_public_values.len(),
        "vkeys and public_values length mismatch"
    );

    let n = vkeys.len() as u32;
    let mut output_hashes: Vec<[u8; 32]> = Vec::with_capacity(vkeys.len());

    for (vkey, public_values) in vkeys.iter().zip(all_public_values.iter()) {
        // Compute the public values digest the same way SP1 does internally:
        // sha256(public_values)
        let digest: [u8; 32] = Sha256::digest(public_values).into();

        // Verify the compressed proof recursively.
        // The proof is NOT read here — it is automatically consumed from the
        // proof input stream by the prover (fed via stdin.write_proof()).
        sp1_zkvm::lib::verify::verify_sp1_proof(vkey, &digest);

        // Extract output_hash from public values [64..96]
        assert!(
            public_values.len() >= 96,
            "public_values too short: {}",
            public_values.len()
        );
        let output_hash: [u8; 32] = public_values[64..96].try_into().unwrap();
        output_hashes.push(output_hash);
    }

    // Compute merkle root over all output_hashes
    let root = merkle_root(&output_hashes);

    // Commit public outputs
    sp1_zkvm::io::commit(&root);
    sp1_zkvm::io::commit(&n);
}

/// Simple merkle root: sha256 of all hashes concatenated.
/// For production use a proper binary merkle tree.
fn merkle_root(hashes: &[[u8; 32]]) -> [u8; 32] {
    if hashes.len() == 1 {
        return hashes[0];
    }
    let mut combined = Vec::with_capacity(hashes.len() * 32);
    for h in hashes {
        combined.extend_from_slice(h);
    }
    Sha256::digest(&combined).into()
}