pub fn get_bytes() -> Vec<u8> {
    include_bytes!("./regtest_mainnet_subminer_auth.wasm").to_vec()
}
