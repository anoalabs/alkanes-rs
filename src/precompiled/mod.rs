pub mod alkanes_std_auth_token_build;
pub mod alkanes_std_beacon_proxy_build;
pub mod alkanes_std_diesel_v3_mainnet_build;
pub mod alkanes_std_diesel_v3_regtest_build;
pub mod alkanes_std_diesel_v3_testnet_build;
pub mod alkanes_std_genesis_alkane_bellscoin_build;
pub mod alkanes_std_genesis_alkane_dogecoin_build;
pub mod alkanes_std_genesis_alkane_fractal_build;
pub mod alkanes_std_genesis_alkane_luckycoin_build;
pub mod alkanes_std_genesis_alkane_mainnet_build;
pub mod alkanes_std_genesis_alkane_regtest_build;
pub mod alkanes_std_genesis_alkane_upgraded_eoa_mainnet_build;
pub mod alkanes_std_genesis_alkane_upgraded_eoa_regtest_build;
pub mod alkanes_std_genesis_alkane_upgraded_mainnet_build;
pub mod alkanes_std_genesis_alkane_upgraded_regtest_build;
pub mod alkanes_std_owned_token_build;
pub mod alkanes_std_proxy_build;
pub mod alkanes_std_upgradeable_beacon_build;
pub mod alkanes_std_upgradeable_build;
pub mod fr_btc_build;
pub mod fr_btc_build_v1_1_0;
pub mod fr_btc_build_v1_2_0;
pub mod fr_btc_build_v1_3_0;
pub mod fr_btc_build_v1_3_1;
pub mod fr_sigil_build;
pub mod free_mint_build;
// regtest_frsigil: the exact MAINNET bytecode of DIESEL v3's governance set
// (fetched with `getbytecode`; byte-identical to the copies under
// src/tests/static/), so the regtest chain runs the same gate, treasury,
// payee token and cFIRE-SIGIL mainnet does. Only compiled into that build.
#[cfg(feature = "regtest_frsigil")]
pub mod regtest_mainnet_cfire_sigil_build;
#[cfg(feature = "regtest_frsigil")]
pub mod regtest_mainnet_subminer_auth_build;
#[cfg(feature = "regtest_frsigil")]
pub mod regtest_mainnet_subminer_gate_build;
#[cfg(feature = "regtest_frsigil")]
pub mod regtest_mainnet_subminer_treasury_build;
