//! Pin a model artifact to IPFS via Pinata.
//!
//! Usage:
//!   PINATA_JWT=<jwt> \
//!   PINATA_GATEWAY_URL=green-gigantic-muskox-661.mypinata.cloud \
//!   cargo run --bin pin_model -- \
//!     --file    prover/artifacts/tiny_mlp_v1.onnx \
//!     --name    tiny_mlp_v1 \
//!     --version 0.1.0

use bytes::Bytes;
use ipfs::{PinMeta, PinataClient};
use std::path::PathBuf;

#[derive(Debug)]
struct Args {
    file: PathBuf,
    name: String,
    version: String,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut file = None;
    let mut name = None;
    let mut version = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--file" => file = args.next().map(PathBuf::from),
            "--name" => name = args.next(),
            "--version" => version = args.next(),
            other => eprintln!("unknown arg: {other}"),
        }
    }

    Args {
        file: file.expect("--file <path> is required"),
        name: name.expect("--name <name> is required"),
        version: version.expect("--version <semver> is required"),
    }
}

#[tokio::main]
async fn main() {
    // Load .env if present (for local dev convenience)
    let _ = dotenvy::dotenv();

    let args = parse_args();

    // Read model file
    let bytes = std::fs::read(&args.file).unwrap_or_else(|e| {
        eprintln!("error: could not read {:?}: {e}", args.file);
        std::process::exit(1);
    });

    let filename = args
        .file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("model.onnx")
        .to_string();

    eprintln!("Pinning  : {filename} ({} bytes)", bytes.len());
    eprintln!("Model    : {} v{}", args.name, args.version);

    let client = PinataClient::from_env().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    let meta = PinMeta {
        name: format!("{}@{}", args.name, args.version),
        keyvalues: Some(serde_json::json!({
            "model_name":    args.name,
            "model_version": args.version,
        })),
    };

    match client
        .pin_bytes(Bytes::from(bytes), filename, Some(meta))
        .await
    {
        Ok(cid) => {
            let url = client.gateway_url(&cid);
            // Print CID to stdout so it can be captured by shell scripts
            println!("{cid}");
            eprintln!("Gateway  : {url}");
            eprintln!("");
            eprintln!("Next steps:");
            eprintln!("  export MODEL_IPFS_CID={cid}");
            eprintln!(
                "  export MODEL_NAME={}",
                args.name.replace(' ', "_").to_lowercase()
            );
            eprintln!("  export MODEL_VERSION={}", "0.1.0");
            eprintln!(
                "  export MODEL_INPUT_SHAPE=$(cast abi-encode 'f(uint256[])' '[1,4]' | cut -c3-)"
            );
            eprintln!("  forge script script/Deploy.s.sol:Deploy --sig 'deployInference()' ...");
        }
        Err(e) => {
            eprintln!("error: pinning failed: {e}");
            std::process::exit(1);
        }
    }
}
