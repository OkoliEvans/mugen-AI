/// Two-layer MLP. MVP demo only — not for production use.
/// Input:  4 f32 features
/// Hidden: 8 neurons, ReLU
/// Output: 2 logits (caller applies softmax if needed)
///
/// Weights layout (flat &[f32], len = WEIGHTS_LEN = 58):
///   [0..32]  layer1 weights (4×8)
///   [32..40] layer1 biases (8)
///   [40..56] layer2 weights (8×2)
///   [56..58] layer2 biases (2)

pub const WEIGHTS_LEN: usize = 58;

pub fn forward(weights: &[f32], input: &[f32]) -> Vec<f32> {
    assert_eq!(weights.len(), WEIGHTS_LEN);
    assert_eq!(input.len(), 4);

    let mut hidden = vec![0f32; 8];
    for i in 0..8 {
        let mut v = weights[32 + i];
        for j in 0..4 {
            v += weights[i * 4 + j] * input[j];
        }
        hidden[i] = v.max(0.0); // ReLU
    }

    let mut output = vec![0f32; 2];
    for i in 0..2 {
        let mut v = weights[56 + i];
        for j in 0..8 {
            v += weights[40 + i * 8 + j] * hidden[j];
        }
        output[i] = v;
    }
    output
}

pub fn weights_to_bytes(weights: &[f32]) -> Vec<u8> {
    weights.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_produces_two_outputs() {
        let w = vec![0.1f32; WEIGHTS_LEN];
        assert_eq!(forward(&w, &[0.5, 0.3, 0.8, 0.1]).len(), 2);
    }

    #[test]
    fn forward_is_deterministic() {
        let w = vec![0.1f32; WEIGHTS_LEN];
        let i = vec![0.5, 0.3, 0.8, 0.1];
        assert_eq!(forward(&w, &i), forward(&w, &i));
    }
}
