pub fn get_bytes() -> Vec<u8> {
    include_bytes!("./regtest_mainnet_subminer_gate.wasm").to_vec()
}
