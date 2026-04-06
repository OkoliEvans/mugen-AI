fn main() {
    let weights = vec![0.1f32; tiny_mlp::WEIGHTS_LEN];
    let bytes: Vec<u8> = weights.iter().flat_map(|f| f.to_le_bytes()).collect();
    std::fs::create_dir_all("weights").unwrap();
    std::fs::write("weights/tiny_mlp.bin", &bytes).unwrap();
    println!("wrote {} weights to weights/tiny_mlp.bin", weights.len());
}