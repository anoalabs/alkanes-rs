pub fn get_bytes() -> Vec<u8> {
    include_bytes!("./regtest_mainnet_cfire_sigil.wasm").to_vec()
}
