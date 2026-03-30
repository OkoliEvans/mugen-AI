use alloy::sol;

// Generate type-safe bindings from the InferenceVerifier ABI inline.
// Alloy's sol! macro generates a fully typed client — no manual ABI encoding.
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    InferenceVerifier,
    r#"[
        {
            "type": "function",
            "name": "submitProof",
            "inputs": [
                { "name": "proof",      "type": "bytes",     "internalType": "bytes" },
                { "name": "instances",  "type": "uint256[]", "internalType": "uint256[]" },
                { "name": "modelId",    "type": "bytes32",   "internalType": "bytes32" },
                { "name": "inputHash",  "type": "bytes32",   "internalType": "bytes32" },
                { "name": "outputHash", "type": "bytes32",   "internalType": "bytes32" }
            ],
            "outputs": [],
            "stateMutability": "nonpayable"
        },
        {
            "type": "function",
            "name": "registerModel",
            "inputs": [
                { "name": "modelId",        "type": "bytes32", "internalType": "bytes32" },
                { "name": "ipfsCid",        "type": "string",  "internalType": "string" },
                { "name": "inputShapeHash", "type": "bytes32", "internalType": "bytes32" }
            ],
            "outputs": [],
            "stateMutability": "nonpayable"
        },
        {
            "type": "function",
            "name": "isVerified",
            "inputs": [
                { "name": "outputHash", "type": "bytes32", "internalType": "bytes32" }
            ],
            "outputs": [
                { "name": "", "type": "bool", "internalType": "bool" }
            ],
            "stateMutability": "view"
        },
        {
            "type": "function",
            "name": "isRegisteredModel",
            "inputs": [
                { "name": "modelId", "type": "bytes32", "internalType": "bytes32" }
            ],
            "outputs": [
                { "name": "", "type": "bool", "internalType": "bool" }
            ],
            "stateMutability": "view"
        },
        {
            "type": "event",
            "name": "InferenceVerified",
            "inputs": [
                { "name": "outputHash", "type": "bytes32", "indexed": true },
                { "name": "modelId",    "type": "bytes32", "indexed": true },
                { "name": "settler",    "type": "address", "indexed": true },
                { "name": "timestamp",  "type": "uint256", "indexed": false }
            ],
            "anonymous": false
        },
        {
            "type": "event",
            "name": "ModelRegistered",
            "inputs": [
                { "name": "modelId",        "type": "bytes32", "indexed": true },
                { "name": "ipfsCid",        "type": "string",  "indexed": false },
                { "name": "ipfsCidHash",    "type": "bytes32", "indexed": true },
                { "name": "inputShapeHash", "type": "bytes32", "indexed": false },
                { "name": "registeredBy",   "type": "address", "indexed": true }
            ],
            "anonymous": false
        },
        {
            "type": "error",
            "name": "NotSettler",
            "inputs": [{ "name": "caller", "type": "address" }]
        },
        {
            "type": "error",
            "name": "AlreadyVerified",
            "inputs": [{ "name": "outputHash", "type": "bytes32" }]
        },
        {
            "type": "error",
            "name": "ModelAlreadyRegistered",
            "inputs": [{ "name": "modelId", "type": "bytes32" }]
        },
        {
            "type": "error",
            "name": "ModelNotRegistered",
            "inputs": [{ "name": "modelId", "type": "bytes32" }]
        },
        {
            "type": "error",
            "name": "InvalidProof",
            "inputs": []
        },
        {
            "type": "error",
            "name": "ZeroAddress",
            "inputs": []
        },
        {
            "type": "error",
            "name": "EmptyString",
            "inputs": []
        }
    ]"#
);

/// Parsed contents of an EZKL proof.json file.
#[derive(Debug, Clone)]
pub struct ParsedProof {
    /// Raw proof bytes to pass to submitProof()
    pub proof: Vec<u8>,
    /// Public instances (model outputs as field elements)
    pub instances: Vec<alloy::primitives::U256>,
}

impl ParsedProof {
    /// Parse an EZKL 23.x proof.json file at the given path.
    ///
    /// EZKL 23.x proof.json structure:
    /// {
    ///   "proof": [41, 172, 170, ...],   // array of u8 integers
    ///   "instances": [[                 // nested array of 64-char hex strings
    ///     "0d00000000000000000000000000000000000000000000000000000000000000",
    ///     ...
    ///   ]]
    /// }
    ///
    /// Instance hex strings are 32-byte little-endian field elements.
    /// They must be byte-reversed before parsing as big-endian U256.
    /// e.g. "0d000000...00" (LE) → reversed → "00...000d" (BE) = U256(13)
    pub fn from_file(path: &str) -> Result<Self, crate::error::SettlerError> {
        use crate::error::SettlerError;

        let raw = std::fs::read_to_string(path)
            .map_err(|_| SettlerError::ProofNotFound(path.to_string()))?;

        let json: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| SettlerError::ProofParseError(e.to_string()))?;

        // proof is an array of u8 integers
        let proof = json["proof"]
            .as_array()
            .ok_or_else(|| SettlerError::ProofParseError("missing 'proof' field".into()))?
            .iter()
            .map(|v| {
                v.as_u64()
                    .map(|n| n as u8)
                    .ok_or_else(|| {
                        SettlerError::ProofParseError(
                            format!("proof byte not a number: {v}")
                        )
                    })
            })
            .collect::<Result<Vec<u8>, _>>()?;

        // instances: nested array of 64-char little-endian hex strings
        let instances_raw = json["instances"]
            .as_array()
            .and_then(|outer| outer.first())
            .and_then(|inner| inner.as_array())
            .ok_or_else(|| {
                SettlerError::ProofParseError("missing 'instances' field".into())
            })?;

        let instances = instances_raw
            .iter()
            .map(|v| {
                let s = v.as_str().unwrap_or_default();
                let s = s.trim_start_matches("0x");

                // Parse hex string into bytes
                let mut bytes = (0..s.len())
                    .step_by(2)
                    .map(|i| {
                        u8::from_str_radix(&s[i..i + 2], 16)
                            .map_err(|e| SettlerError::ProofParseError(e.to_string()))
                    })
                    .collect::<Result<Vec<u8>, _>>()?;

                // Reverse: little-endian → big-endian for U256 parsing
                bytes.reverse();

                alloy::primitives::U256::from_be_slice(&bytes)
                    .try_into()
                    .map_err(|_| SettlerError::ProofParseError(
                        "U256 conversion failed".into()
                    ))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { proof, instances })
    }
}