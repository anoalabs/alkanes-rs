use crate::message::AlkaneMessageContext;
use crate::precompiled::fr_btc_build_v1_1_0;
use crate::precompiled::fr_btc_build_v1_2_0;
use crate::precompiled::fr_btc_build_v1_3_0;
use crate::precompiled::fr_btc_build_v1_3_1;
#[allow(unused_imports)]
use crate::precompiled::{
    alkanes_std_diesel_v3_mainnet_build, alkanes_std_diesel_v3_regtest_build,
    alkanes_std_diesel_v3_testnet_build,
    alkanes_std_genesis_alkane_dogecoin_build, alkanes_std_genesis_alkane_fractal_build,
    alkanes_std_genesis_alkane_luckycoin_build, alkanes_std_genesis_alkane_mainnet_build,
    alkanes_std_genesis_alkane_regtest_build,
    alkanes_std_genesis_alkane_upgraded_eoa_mainnet_build,
    alkanes_std_genesis_alkane_upgraded_eoa_regtest_build,
    alkanes_std_genesis_alkane_upgraded_mainnet_build,
    alkanes_std_genesis_alkane_upgraded_regtest_build, fr_btc_build, fr_sigil_build,
};
use crate::utils::pipe_storagemap_to;
use crate::view::simulate_parcel;
use crate::vm::utils::sequence_pointer;
use alkanes_support::cellpack::Cellpack;
use alkanes_support::gz::compress;
use alkanes_support::id::AlkaneId;
use alkanes_support::parcel::AlkaneTransferParcel;
use anyhow::Result;
use bitcoin::{Block, OutPoint, Transaction};
use metashrew_core::index_pointer::{AtomicPointer, IndexPointer};
use metashrew_support::index_pointer::KeyValuePointer;
use protorune::balance_sheet::PersistentRecord;
use protorune::message::{MessageContext, MessageContextParcel};
#[allow(unused_imports)]
use protorune::tables::{RuneTable, RUNES};
use protorune_support::balance_sheet::BalanceSheet;
use protorune_support::utils::{outpoint_encode, tx_hex_to_txid};
use std::sync::Arc;

#[allow(unused_imports)]
use {
    metashrew_core::{println, stdio::stdout},
    std::fmt::Write,
};

pub fn fr_btc_bytes() -> Vec<u8> {
    // On chains where V220 is active from genesis (regtest + alt-coin
    // networks where V220_FORK_HEIGHT=0), `setup_frbtc` must initialize
    // against the slim binary directly — initializing bulky and then
    // executing slim would leave slim running against an unfamiliar
    // storage layout, and it fuel-exhausts.
    // For mainnet (V220_FORK_HEIGHT=950_000 > 0), this returns bulky to
    // replicate the historical setup at block 880_000.
    if genesis::V220_FORK_HEIGHT == 0 {
        // Genesis-coincident chains (regtest + alt-coins): use the latest
        // slim build straight away. v1.3.1 is storage-init compatible with the
        // v1.2.0/v1.3.0 slim builds these chains already ran against, so this
        // preserves their post-V220 behaviour and matches frbtc_wasm_for_height
        // (FRBTC_V131_FORK_HEIGHT=0 on these chains → v1.3.1 executes).
        fr_btc_build_v1_3_1::get_bytes()
    } else {
        fr_btc_build::get_bytes()
    }
}

/// Height-versioned static frBTC (`32:0`) code map — the "load a precompiled
/// built-in directly from static program storage" path.
///
/// frBTC code is resolved by block height from bytes compiled into this binary,
/// instead of read from indexed state. A binary upgrade that adds a new version
/// + fork height therefore activates on every node at that height without a
/// state migration, and can never be left inert on a rolled pod that already
/// indexed past the fork height.
///
/// Boundaries are `>=` and mirror the historical one-shot byte-swaps that used
/// to rewrite indexed state at these heights (each swap ran *before* tx
/// indexing of its activation block), so the bytes returned here are
/// byte-identical to what execution historically saw at every height.
/// The `else` arm reuses `fr_btc_bytes()`: bulky at mainnet genesis, and the
/// unreachable-on-genesis-coincident-chains fallback (there `FRBTC_V130 == 0`
/// so the first arm always matches).
pub fn frbtc_wasm_for_height(height: u32) -> Vec<u8> {
    if height >= genesis::FRBTC_V131_FORK_HEIGHT {
        // v1.3.1: caches the tweaked P2TR signer script (skips the ~2M
        // tap_tweak per wrap that starved chained wrap+swap under v1.3.0) +
        // unconditional owner-gated set_signer. Mainnet fork @ 960_000.
        fr_btc_build_v1_3_1::get_bytes()
    } else if height >= genesis::FRBTC_V130_FORK_HEIGHT {
        fr_btc_build_v1_3_0::get_bytes()
    } else if height >= genesis::V220_FORK_HEIGHT {
        fr_btc_build_v1_2_0::get_bytes()
    } else if height >= genesis::GENESIS_UPGRADE_EOA_BLOCK_HEIGHT {
        fr_btc_build_v1_1_0::get_bytes()
    } else {
        fr_btc_bytes()
    }
}

pub fn fr_sigil_bytes() -> Vec<u8> {
    fr_sigil_build::get_bytes()
}

#[cfg(feature = "mainnet")]
pub fn genesis_alkane_bytes() -> Vec<u8> {
    alkanes_std_genesis_alkane_mainnet_build::get_bytes()
}

//use if regtest
#[cfg(all(
    not(feature = "mainnet"),
    not(feature = "dogecoin"),
    not(feature = "bellscoin"),
    not(feature = "fractal"),
    not(feature = "luckycoin")
))]
pub fn genesis_alkane_bytes() -> Vec<u8> {
    alkanes_std_genesis_alkane_regtest_build::get_bytes()
}

#[cfg(feature = "dogecoin")]
pub fn genesis_alkane_bytes() -> Vec<u8> {
    alkanes_std_genesis_alkane_dogecoin_build::get_bytes()
}

#[cfg(feature = "bellscoin")]
pub fn genesis_alkane_bytes() -> Vec<u8> {
    alkanes_std_genesis_alkane_dogecoin_build::get_bytes()
}

#[cfg(feature = "fractal")]
pub fn genesis_alkane_bytes() -> Vec<u8> {
    alkanes_std_genesis_alkane_fractal_build::get_bytes()
}

#[cfg(feature = "luckycoin")]
pub fn genesis_alkane_bytes() -> Vec<u8> {
    alkanes_std_genesis_alkane_luckycoin_build::get_bytes()
}

#[cfg(feature = "mainnet")]
pub fn genesis_alkane_upgrade_bytes() -> Vec<u8> {
    alkanes_std_genesis_alkane_upgraded_mainnet_build::get_bytes()
}

//use if regtest
#[cfg(all(
    not(feature = "mainnet"),
    not(feature = "dogecoin"),
    not(feature = "bellscoin"),
    not(feature = "fractal"),
    not(feature = "luckycoin")
))]
pub fn genesis_alkane_upgrade_bytes() -> Vec<u8> {
    alkanes_std_genesis_alkane_upgraded_regtest_build::get_bytes()
}

#[cfg(feature = "mainnet")]
pub fn genesis_alkane_upgrade_bytes_eoa() -> Vec<u8> {
    alkanes_std_genesis_alkane_upgraded_eoa_mainnet_build::get_bytes()
}

//use if regtest
#[cfg(all(
    not(feature = "mainnet"),
    not(feature = "dogecoin"),
    not(feature = "bellscoin"),
    not(feature = "fractal"),
    not(feature = "luckycoin")
))]
pub fn genesis_alkane_upgrade_bytes_eoa() -> Vec<u8> {
    alkanes_std_genesis_alkane_upgraded_eoa_regtest_build::get_bytes()
}

#[cfg(feature = "mainnet")]
pub fn diesel_v3_bytes() -> Vec<u8> {
    alkanes_std_diesel_v3_mainnet_build::get_bytes()
}

/// testnet4: DIESEL v3 built with `V3_ACTIVATION_HEIGHT = 0` to match
/// `genesis::DIESEL_V3_BLOCK_HEIGHT` below.
#[cfg(all(feature = "testnet", not(feature = "mainnet")))]
pub fn diesel_v3_bytes() -> Vec<u8> {
    alkanes_std_diesel_v3_testnet_build::get_bytes()
}

//use if regtest
#[cfg(all(
    not(feature = "mainnet"),
    not(feature = "testnet"),
    not(feature = "regtest_frsigil"),
    not(feature = "dogecoin"),
    not(feature = "bellscoin"),
    not(feature = "fractal"),
    not(feature = "luckycoin")
))]
pub fn diesel_v3_bytes() -> Vec<u8> {
    alkanes_std_diesel_v3_regtest_build::get_bytes()
}

/// regtest_frsigil: DIESEL v3 is live from genesis, so `2:0` runs the build
/// whose `V3_ACTIVATION_HEIGHT` is 0 (the testnet artifact: regtest chain
/// configuration, activation at 0), matching `genesis::DIESEL_V3_BLOCK_HEIGHT`
/// below. The stock regtest artifact activates at 2_000_000 and would leave
/// the emission ledger unopened until then.
#[cfg(all(feature = "regtest_frsigil", not(feature = "mainnet"), not(feature = "testnet")))]
pub fn diesel_v3_bytes() -> Vec<u8> {
    alkanes_std_diesel_v3_testnet_build::get_bytes()
}

/// Height-versioned static genesis-alkane / DIESEL (`2:0`) code map.
///
/// Boundaries are `>=` and mirror the historical one-shot byte-swaps that used
/// to rewrite indexed state at `GENESIS_UPGRADE_BLOCK_HEIGHT` and
/// `GENESIS_UPGRADE_EOA_BLOCK_HEIGHT` (each ran *before* tx indexing of its
/// activation block), so the bytes returned here are byte-identical to what
/// execution historically saw at every height.
pub fn genesis_alkane_wasm_for_height(height: u32) -> Vec<u8> {
    if height >= genesis::DIESEL_V3_BLOCK_HEIGHT {
        diesel_v3_bytes()
    } else if height >= genesis::GENESIS_UPGRADE_EOA_BLOCK_HEIGHT {
        genesis_alkane_upgrade_bytes_eoa()
    } else if height >= genesis::GENESIS_UPGRADE_BLOCK_HEIGHT {
        genesis_alkane_upgrade_bytes()
    } else {
        genesis_alkane_bytes()
    }
}

/// The single load path for precompiled built-ins: resolves the alkane's code
/// for `height` from the static in-binary version maps, or `None` when the id
/// is not a precompile. Precompiled code is never read from (or written to)
/// the `/alkanes/` bytecode table.
pub fn precompiled_alkane_wasm_for_height(id: &AlkaneId, height: u32) -> Option<Vec<u8>> {
    if *id == (AlkaneId { block: 2, tx: 0 }) {
        Some(genesis_alkane_wasm_for_height(height))
    } else if *id == (AlkaneId { block: 32, tx: 0 }) {
        Some(frbtc_wasm_for_height(height))
    } else if *id == (AlkaneId { block: 32, tx: 1 }) {
        Some(fr_sigil_bytes())
    } else {
        None
    }
}

//use if regtest
#[cfg(all(
    not(feature = "mainnet"),
    not(feature = "testnet"),
    not(feature = "dogecoin"),
    not(feature = "bellscoin"),
    not(feature = "fractal"),
    not(feature = "luckycoin")
))]
pub mod genesis {
    pub const GENESIS_BLOCK: u64 = 0;
    pub const GENESIS_OUTPOINT: &str =
        "3977b30a97c9b9d609afb4b7cc138e17b21d1e0c5e360d25debf1441de933bf4";
    pub const GENESIS_OUTPOINT_BLOCK_HEIGHT: u64 = 0;
    pub const GENESIS_UPGRADE_BLOCK_HEIGHT: u32 = 0;
    pub const GENESIS_UPGRADE_EOA_BLOCK_HEIGHT: u32 = 0;
    /// DIESEL v3 fork: activates `alkanes-std-diesel-v3` at `2:0` — the
    /// governance-set mint gate, and the new default where an ungated mint
    /// accrues to the contract's claimable fees for the DSIGIL owner instead of
    /// paying the caller.
    ///
    /// Deliberately NOT genesis-coincident on regtest, unlike the other forks
    /// here: the existing DIESEL suite asserts the pre-v3 pay-the-minter
    /// behaviour, and some of it indexes at realistic mainnet-ish heights
    /// (`tests::merkle_distributor` mints at 840_001,
    /// `tests::genesis_upgrade` at 890_000). This sits above all of them so
    /// those tests keep exercising the eoa binary, while `tests::mint_gate`
    /// indexes past it to opt into v3. Keep it under 2_100_000 so the halving
    /// schedule in `block_reward` still yields a non-zero reward.
    #[cfg(not(feature = "regtest_frsigil"))]
    pub const DIESEL_V3_BLOCK_HEIGHT: u32 = 2_000_000;
    /// regtest_frsigil (B8's chain): DIESEL v3 from genesis, like testnet4.
    /// The pre-v3 `2:0` binaries never execute; `setup_diesel` initializes v3
    /// directly and `setup_regtest_subminer` installs mainnet's governance set
    /// at block 1 (see `regtest_subminer`).
    #[cfg(feature = "regtest_frsigil")]
    pub const DIESEL_V3_BLOCK_HEIGHT: u32 = 0;
    /// v2.2.0 fork: activates the slim fr_btc.wasm precompile + the
    /// extcall-child-revert containment fix (ports of kungfuflex/v2.1.8 +
    /// kungfuflex/v2.1.8-slim-frbtc). On regtest the fork is genesis-coincident
    /// so all tests run against the post-fork behaviour by default.
    pub const V220_FORK_HEIGHT: u32 = 0;
    /// v2.2.1-alpha.3 fork: activates fr_btc v1.3.0. Genesis-coincident on
    /// non-mainnet chains, so the static frBTC version map resolves to v1.3.0
    /// from genesis here.
    pub const FRBTC_V130_FORK_HEIGHT: u32 = 0;
    pub const FRBTC_V131_FORK_HEIGHT: u32 = 0;
}

/// testnet4 (`--features testnet`): the latest consensus rules from block 0.
/// Unlike regtest, nothing is held back for the test-suite — DIESEL v3 (and
/// with it the 4:1001 mint gate and cFIRE-SIGIL 4:866 governance) is live at
/// genesis, as are every protorune/fuel/frBTC fork (all 0 off mainnet).
/// Pre-v3 `2:0` binaries are never executed: `genesis_alkane_wasm_for_height`
/// resolves v3 at every height, so `setup_diesel` initializes v3 directly.
#[cfg(all(feature = "testnet", not(feature = "mainnet")))]
pub mod genesis {
    pub const GENESIS_BLOCK: u64 = 0;
    pub const GENESIS_OUTPOINT: &str =
        "3977b30a97c9b9d609afb4b7cc138e17b21d1e0c5e360d25debf1441de933bf4";
    pub const GENESIS_OUTPOINT_BLOCK_HEIGHT: u64 = 0;
    pub const GENESIS_UPGRADE_BLOCK_HEIGHT: u32 = 0;
    pub const GENESIS_UPGRADE_EOA_BLOCK_HEIGHT: u32 = 0;
    pub const DIESEL_V3_BLOCK_HEIGHT: u32 = 0;
    pub const V220_FORK_HEIGHT: u32 = 0;
    pub const FRBTC_V130_FORK_HEIGHT: u32 = 0;
    pub const FRBTC_V131_FORK_HEIGHT: u32 = 0;
}

#[cfg(feature = "mainnet")]
pub mod genesis {
    pub const GENESIS_BLOCK: u64 = 880_000;
    pub const GENESIS_OUTPOINT: &str =
        "3977b30a97c9b9d609afb4b7cc138e17b21d1e0c5e360d25debf1441de933bf4";
    pub const GENESIS_OUTPOINT_BLOCK_HEIGHT: u64 = 872_101;
    pub const GENESIS_UPGRADE_BLOCK_HEIGHT: u32 = 908_888;

    pub const GENESIS_UPGRADE_EOA_BLOCK_HEIGHT: u32 = 917_888;
    /// DIESEL v3 mainnet fork: activates `alkanes-std-diesel-v3` at `2:0` — the
    /// governance-set mint gate (opcode 80 / audit view 102), and the new
    /// default where an ungated mint accrues to the contract's claimable fees
    /// for the DSIGIL owner (opcode 78, `only_owner`) instead of paying the
    /// caller. Future block — coordinated hard fork; all indexers MUST ship this
    /// activation before this height or they diverge at 2:0.
    pub const DIESEL_V3_BLOCK_HEIGHT: u32 = 972_000;
    /// v2.2.0 mainnet fork: slim fr_btc.wasm + extcall revert containment.
    pub const V220_FORK_HEIGHT: u32 = 950_000;
    /// v2.2.1-alpha.3 mainnet fork: activates fr_btc v1.3.0. Future block —
    /// coordinated hard fork; all indexers MUST ship this activation (roll
    /// v2.2.1-alpha.3) before this height or they diverge at 32:0.
    pub const FRBTC_V130_FORK_HEIGHT: u32 = 957_000;
    pub const FRBTC_V131_FORK_HEIGHT: u32 = 960_000;
}

#[cfg(feature = "fractal")]
pub mod genesis {
    pub const GENESIS_BLOCK: u64 = 400_000;
    pub const GENESIS_OUTPOINT: &str =
        "cf2b52ffaaf1c094df22f190b888fb0e474fe62990547a34e144ec9f8e135b07";
    pub const GENESIS_OUTPOINT_BLOCK_HEIGHT: u64 = 228_194;
    pub const GENESIS_UPGRADE_BLOCK_HEIGHT: u32 = 228_194;
    pub const GENESIS_UPGRADE_EOA_BLOCK_HEIGHT: u32 = 228_194;
    pub const DIESEL_V3_BLOCK_HEIGHT: u32 = 0;
    pub const V220_FORK_HEIGHT: u32 = 0;
    /// v2.2.1-alpha.3 fork: activates fr_btc v1.3.0. Genesis-coincident on
    /// non-mainnet chains, so the static frBTC version map resolves to v1.3.0
    /// from genesis here.
    pub const FRBTC_V130_FORK_HEIGHT: u32 = 0;
    pub const FRBTC_V131_FORK_HEIGHT: u32 = 0;
}

#[cfg(feature = "dogecoin")]
pub mod genesis {
    pub const GENESIS_BLOCK: u64 = 6_000_000;
    pub const GENESIS_OUTPOINT: &str =
        "cf2b52ffaaf1c094df22f190b888fb0e474fe62990547a34e144ec9f8e135b07";
    pub const GENESIS_OUTPOINT_BLOCK_HEIGHT: u64 = 872_101;
    pub const GENESIS_UPGRADE_BLOCK_HEIGHT: u32 = 872_101;
    pub const GENESIS_UPGRADE_EOA_BLOCK_HEIGHT: u32 = 872_101;
    pub const DIESEL_V3_BLOCK_HEIGHT: u32 = 0;
    pub const V220_FORK_HEIGHT: u32 = 0;
    /// v2.2.1-alpha.3 fork: activates fr_btc v1.3.0. Genesis-coincident on
    /// non-mainnet chains, so the static frBTC version map resolves to v1.3.0
    /// from genesis here.
    pub const FRBTC_V130_FORK_HEIGHT: u32 = 0;
    pub const FRBTC_V131_FORK_HEIGHT: u32 = 0;
}

#[cfg(feature = "luckycoin")]
pub mod genesis {
    pub const GENESIS_BLOCK: u64 = 400_000;
    pub const GENESIS_OUTPOINT: &str =
        "cf2b52ffaaf1c094df22f190b888fb0e474fe62990547a34e144ec9f8e135b07";
    pub const GENESIS_OUTPOINT_BLOCK_HEIGHT: u64 = 872_101;
    pub const GENESIS_UPGRADE_BLOCK_HEIGHT: u32 = 872_101;
    pub const GENESIS_UPGRADE_EOA_BLOCK_HEIGHT: u32 = 872_101;
    pub const DIESEL_V3_BLOCK_HEIGHT: u32 = 0;
    pub const V220_FORK_HEIGHT: u32 = 0;
    /// v2.2.1-alpha.3 fork: activates fr_btc v1.3.0. Genesis-coincident on
    /// non-mainnet chains, so the static frBTC version map resolves to v1.3.0
    /// from genesis here.
    pub const FRBTC_V130_FORK_HEIGHT: u32 = 0;
    pub const FRBTC_V131_FORK_HEIGHT: u32 = 0;
}

#[cfg(feature = "bellscoin")]
pub mod genesis {
    pub const GENESIS_BLOCK: u64 = 500_000;
    pub const GENESIS_OUTPOINT: &str =
        "2c58484a86e117a445c547d8f3acb56b569f7ea036637d909224d52a5b990259";
    pub const GENESIS_OUTPOINT_BLOCK_HEIGHT: u64 = 288_906;
    pub const GENESIS_UPGRADE_BLOCK_HEIGHT: u32 = 288_906;
    pub const GENESIS_UPGRADE_EOA_BLOCK_HEIGHT: u32 = 288_906;
    pub const DIESEL_V3_BLOCK_HEIGHT: u32 = 0;
    pub const V220_FORK_HEIGHT: u32 = 0;
    /// v2.2.1-alpha.3 fork: activates fr_btc v1.3.0. Genesis-coincident on
    /// non-mainnet chains, so the static frBTC version map resolves to v1.3.0
    /// from genesis here.
    pub const FRBTC_V130_FORK_HEIGHT: u32 = 0;
    pub const FRBTC_V131_FORK_HEIGHT: u32 = 0;
}

pub fn is_active(height: u64) -> bool {
    height >= genesis::GENESIS_BLOCK
}

static mut _VIEW: bool = false;

pub fn set_view_mode() {
    unsafe {
        _VIEW = true;
    }
}

pub fn get_view_mode() -> bool {
    unsafe { _VIEW }
}

pub fn is_genesis(height: u64) -> bool {
    let mut init_ptr = IndexPointer::from_keyword("/seen-genesis");
    let has_not_seen_genesis = init_ptr.get().len() == 0;
    let is_genesis = if has_not_seen_genesis {
        get_view_mode() || height >= genesis::GENESIS_BLOCK
    } else {
        false
    };
    if is_genesis {
        init_ptr.set_value::<u8>(0x01);
    }
    is_genesis
}

/// On the `regtest_frsigil` build the 1-unit frSIGIL auth-token premine is
/// deferred out of genesis and re-homed onto the coinbase of this height, a
/// b8-controlled, bitcoin-spendable regtest outpoint (driven from
/// `index_block` via `premine_frsigil`). Stock builds premine at genesis to
/// the fixed `GENESIS_OUTPOINT`, which is a phantom (unspendable) outpoint on
/// regtest, so frSIGIL can never be moved to authorize `set_signer`. The same
/// coinbase receives the cFIRE-SIGIL and SUBMINER-AUTH units
/// (`setup_regtest_subminer`), so one key governs frBTC, DIESEL's mint gate
/// and the SUBMINER treasury.
#[cfg(feature = "regtest_frsigil")]
pub const FRSIGIL_PREMINE_HEIGHT: u32 = 1;

pub fn setup_frsigil(block: &Block) -> Result<()> {
    // Byte presence at /alkanes/32:1 is the deployed-detection marker (kept
    // for backwards compatibility with already-synced DBs). Execution never
    // reads these bytes — `get_alkane_binary` resolves `32:1` from the static
    // in-binary code via `precompiled_alkane_wasm_for_height`.
    let mut ptr =
        IndexPointer::from_keyword("/alkanes/").select(&(AlkaneId { block: 32, tx: 1 }).into());
    if ptr.get().len() == 0 {
        ptr.set(Arc::new(compress(fr_sigil_bytes())?));
    } else {
        return Ok(());
    }
    // Stock: premine the frSIGIL auth token to the fixed genesis outpoint
    // immediately. On `regtest_frsigil` the premine is deferred to
    // `FRSIGIL_PREMINE_HEIGHT`'s coinbase (a spendable regtest UTXO b8 owns),
    // done from `index_block` after protorune's own block indexing.
    #[cfg(not(feature = "regtest_frsigil"))]
    {
        let outpoint = OutPoint {
            txid: tx_hex_to_txid(genesis::GENESIS_OUTPOINT)?,
            vout: 0,
        };
        premine_frsigil(block, outpoint)?;
    }
    let _ = block;
    Ok(())
}

/// Mint the single frSIGIL (32:1) auth-token unit and premine it to
/// `outpoint`. Idempotent: a `/frsigil/premined` flag guards double-premine
/// across re-scans. Split out of `setup_frsigil` so the `regtest_frsigil`
/// build can point the premine at a b8-controlled coinbase outpoint.
pub fn premine_frsigil(block: &Block, outpoint: OutPoint) -> Result<()> {
    let mut flag = IndexPointer::from_keyword("/frsigil/premined");
    if flag.get().len() != 0 {
        return Ok(());
    }
    let mut atomic: AtomicPointer = AtomicPointer::default();
    let fr_sigil = AlkaneId { block: 32, tx: 1 };

    let parcel3 = MessageContextParcel {
        atomic: atomic.derive(&IndexPointer::default()),
        runes: vec![],
        transaction: Transaction {
            version: bitcoin::blockdata::transaction::Version::ONE,
            input: vec![],
            output: vec![],
            lock_time: bitcoin::absolute::LockTime::ZERO,
        },
        block: std::sync::Arc::new(block.clone()),
        height: genesis::GENESIS_BLOCK,
        pointer: 0,
        refund_pointer: 0,
        calldata: (Cellpack {
            target: fr_sigil.clone(),
            inputs: vec![0, 1],
        })
        .encipher(),
        sheets: Box::<BalanceSheet<AtomicPointer>>::new(BalanceSheet::default()),
        txindex: 0,
        vout: 0,
        runtime_balances: Box::<BalanceSheet<AtomicPointer>>::new(BalanceSheet::default()),
    };
    let (response2, _gas_used2) = (match simulate_parcel(&parcel3, u64::MAX) {
        Ok((a, b)) => Ok((a, b)),
        Err(e) => {
            println!("{:?}", e);
            Err(e)
        }
    })?;
    let outpoint_bytes = outpoint_encode(&outpoint)?;
    <AlkaneTransferParcel as TryInto<BalanceSheet<AtomicPointer>>>::try_into(
        response2.alkanes.into(),
    )?
    .save(
        &mut atomic.derive(
            &RuneTable::for_protocol(AlkaneMessageContext::protocol_tag())
                .OUTPOINT_TO_RUNES
                .select(&outpoint_bytes),
        ),
        false,
    );
    pipe_storagemap_to(
        &response2.storage,
        &mut atomic
            .derive(&IndexPointer::from_keyword("/alkanes/").select(&fr_sigil.clone().into())),
    );
    flag.set_value::<u8>(1);
    atomic.commit();
    Ok(())
}

/// regtest_frsigil: mainnet's DIESEL v3 governance set, installed at genesis.
///
/// On mainnet DIESEL v3 (`2:0`, from 972_000) forwards EVERY mint to the
/// governance-set mint gate, SUBMINER-GATE at `4:1001`, which keeps
/// `caller_bp` basis points for the minter and deposits the rest into the
/// SUBMINER treasury at `2:96782`; the treasury pays out to whoever presents
/// its payee token, SUBMINER-AUTH `2:96775`; and the cFIRE-SIGIL `4:866`
/// governs the gate (DIESEL opcode 80 set_mint_gate, 78 collect_fees; gate
/// opcode 2 set_split). This module reproduces that state on B8's regtest
/// chain, with the EXACT mainnet bytecode (`src/precompiled/regtest_mainnet_*`,
/// fetched with `getbytecode`) and the exact mainnet initialize arguments
/// (read from the mainnet deploy traces on 2026-11-21):
///
/// | id        | contract         | initialize                                   |
/// |-----------|------------------|----------------------------------------------|
/// | 4:866     | CfireSigil       | `[0, 1]` (1 unit)                            |
/// | 2:96775   | SubminerPayee    | `[0, 1]` (1 unit, "SUBMINER-AUTH")           |
/// | 2:96782   | SubminerTreasury | `[0, 2,0, 2,96775]` (asset DIESEL, payee)    |
/// | 4:1001    | SubminerGate     | `[0, 2,0, 2,96782, 4,866, 192]` (192 bp)     |
///
/// The contracts are written straight into the `/alkanes/` bytecode table at
/// their mainnet ids and initialized through `simulate_parcel`, the same way
/// `setup_frbtc` installs `32:0`; the sequence counter is left alone. Both
/// capability units land on the `FRSIGIL_PREMINE_HEIGHT` coinbase next to the
/// frSIGIL, so the operator key that governs frBTC also governs DIESEL's gate
/// and can collect from the treasury. DIESEL's `/upgrade_initialized` is set
/// (mainnet crossed the premine upgrade long before v3 retired opcode 1), so
/// mints are one-per-tx and split per block like mainnet's. `/mint-gate` is
/// left empty: the compile-time default 4:1001 is in effect, exactly as on
/// mainnet (source byte 0 in the opcode-102 view). Operators pick a different
/// policy with the cFIRE-SIGIL (clear the gate, or `set_split`), never by
/// changing this genesis.
#[cfg(feature = "regtest_frsigil")]
pub mod regtest_subminer {
    use alkanes_support::id::AlkaneId;

    pub const CFIRE_SIGIL: AlkaneId = AlkaneId { block: 4, tx: 866 };
    pub const SUBMINER_AUTH: AlkaneId = AlkaneId { block: 2, tx: 96775 };
    pub const SUBMINER_TREASURY: AlkaneId = AlkaneId { block: 2, tx: 96782 };
    pub const SUBMINER_GATE: AlkaneId = AlkaneId { block: 4, tx: 1001 };
    pub const DIESEL: AlkaneId = AlkaneId { block: 2, tx: 0 };
    /// Minter's share of a mint in basis points (mainnet: 1.92%).
    pub const CALLER_BP: u128 = 192;
    /// Capability units minted at initialize, as on mainnet.
    pub const CFIRE_SIGIL_UNITS: u128 = 1;
    pub const SUBMINER_AUTH_UNITS: u128 = 1;
}

/// Install one built-in at a fixed id: write its bytecode (compressed, the
/// `/alkanes/` table's format), run `initialize` with `inputs` as a synthetic
/// EOA call at `height`, pipe the resulting storage under the id and, when
/// `premine_to` is given, credit whatever the call minted to that outpoint
/// (merging with alkanes already recorded there). Idempotent on bytecode
/// presence, like `setup_frbtc`.
#[cfg(feature = "regtest_frsigil")]
fn install_builtin(
    block: &Block,
    height: u64,
    id: AlkaneId,
    bytecode: Vec<u8>,
    inputs: Vec<u128>,
    premine_to: Option<OutPoint>,
) -> Result<()> {
    let mut ptr = IndexPointer::from_keyword("/alkanes/").select(&id.clone().into());
    if ptr.get().len() != 0 {
        return Ok(());
    }
    ptr.set(Arc::new(compress(bytecode)?));
    let mut atomic: AtomicPointer = AtomicPointer::default();
    let parcel = MessageContextParcel {
        atomic: atomic.derive(&IndexPointer::default()),
        runes: vec![],
        transaction: Transaction {
            version: bitcoin::blockdata::transaction::Version::ONE,
            input: vec![],
            output: vec![],
            lock_time: bitcoin::absolute::LockTime::ZERO,
        },
        block: std::sync::Arc::new(block.clone()),
        height,
        pointer: 0,
        refund_pointer: 0,
        calldata: (Cellpack {
            target: id.clone(),
            inputs,
        })
        .encipher(),
        sheets: Box::<BalanceSheet<AtomicPointer>>::new(BalanceSheet::default()),
        txindex: 0,
        vout: 0,
        runtime_balances: Box::<BalanceSheet<AtomicPointer>>::new(BalanceSheet::default()),
    };
    let (response, _gas) = (match simulate_parcel(&parcel, u64::MAX) {
        Ok((a, b)) => Ok((a, b)),
        Err(e) => {
            println!("regtest_subminer: initialize of {:?} failed: {:?}", id, e);
            Err(e)
        }
    })?;
    if let Some(outpoint) = premine_to {
        let outpoint_bytes = outpoint_encode(&outpoint)?;
        <AlkaneTransferParcel as TryInto<BalanceSheet<AtomicPointer>>>::try_into(
            response.alkanes.into(),
        )?
        .save(
            &mut atomic.derive(
                &RuneTable::for_protocol(AlkaneMessageContext::protocol_tag())
                    .OUTPOINT_TO_RUNES
                    .select(&outpoint_bytes),
            ),
            false,
        );
    }
    pipe_storagemap_to(
        &response.storage,
        &mut atomic.derive(&IndexPointer::from_keyword("/alkanes/").select(&id.clone().into())),
    );
    atomic.commit();
    Ok(())
}

/// regtest_frsigil: deploy mainnet's DIESEL v3 governance set (see
/// [`regtest_subminer`]) and premine its capability units onto `coinbase`,
/// then preset DIESEL's `/upgrade_initialized`. Called from `index_block` at
/// `FRSIGIL_PREMINE_HEIGHT`, after protorune indexed the block, right after
/// `premine_frsigil`. Idempotent: each contract installs once (bytecode
/// presence), the DIESEL flag is a plain set.
#[cfg(feature = "regtest_frsigil")]
pub fn setup_regtest_subminer(block: &Block, height: u32, coinbase: OutPoint) -> Result<()> {
    use crate::precompiled::{
        regtest_mainnet_cfire_sigil_build, regtest_mainnet_subminer_auth_build,
        regtest_mainnet_subminer_gate_build, regtest_mainnet_subminer_treasury_build,
    };
    use regtest_subminer::*;
    let h = height as u64;
    // The runtime only dispatches to a block-2 id below the sequence counter
    // (`AlkaneId::is_created`); mainnet's counter is far past 2:96782, a fresh
    // regtest's is 1. Advance it past the treasury so the payee and treasury
    // are callable (the gate's mint-split calls the treasury on every mint)
    // and so no later CREATE can land on their ids. New contracts on this
    // chain therefore start at 2:96783; B8 reads ids from the `sequence`
    // view, never assumes them.
    {
        let mut atomic: AtomicPointer = AtomicPointer::default();
        let mut seq = sequence_pointer(&atomic);
        let floor = SUBMINER_TREASURY.tx + 1;
        if seq.get_value::<u128>() < floor {
            seq.set_value::<u128>(floor);
        }
        atomic.commit();
    }
    // deploy order = mainnet order: sigil (965_939), payee (967_712),
    // treasury (967_721), gate (970_251)
    install_builtin(
        block,
        h,
        CFIRE_SIGIL,
        regtest_mainnet_cfire_sigil_build::get_bytes(),
        vec![0, CFIRE_SIGIL_UNITS],
        Some(coinbase),
    )?;
    install_builtin(
        block,
        h,
        SUBMINER_AUTH,
        regtest_mainnet_subminer_auth_build::get_bytes(),
        vec![0, SUBMINER_AUTH_UNITS],
        Some(coinbase),
    )?;
    install_builtin(
        block,
        h,
        SUBMINER_TREASURY,
        regtest_mainnet_subminer_treasury_build::get_bytes(),
        vec![0, DIESEL.block, DIESEL.tx, SUBMINER_AUTH.block, SUBMINER_AUTH.tx],
        None,
    )?;
    install_builtin(
        block,
        h,
        SUBMINER_GATE,
        regtest_mainnet_subminer_gate_build::get_bytes(),
        vec![
            0,
            DIESEL.block,
            DIESEL.tx,
            SUBMINER_TREASURY.block,
            SUBMINER_TREASURY.tx,
            CFIRE_SIGIL.block,
            CFIRE_SIGIL.tx,
            CALLER_BP,
        ],
        None,
    )?;
    // Mainnet inherited `/upgrade_initialized` from the 2025 premine upgrade;
    // v3 retired that opcode, so a chain born on v3 presets the flag here.
    // Without it DIESEL stays in the legacy one-mint-per-BLOCK mode forever.
    IndexPointer::from_keyword("/alkanes/")
        .select(&DIESEL.into())
        .keyword("/storage/")
        .select(&b"/upgrade_initialized".to_vec())
        .set_value::<u8>(0x01);
    Ok(())
}

pub fn setup_frbtc(block: &Block) -> Result<()> {
    // Byte presence at /alkanes/32:0 is the deployed-detection marker (kept
    // for backwards compatibility with already-synced DBs). Execution never
    // reads these bytes — `get_alkane_binary` resolves `32:0` from the static
    // in-binary version map via `precompiled_alkane_wasm_for_height`.
    let mut ptr =
        IndexPointer::from_keyword("/alkanes/").select(&(AlkaneId { block: 32, tx: 0 }).into());
    if ptr.get().len() == 0 {
        ptr.set(Arc::new(compress(fr_btc_bytes())?));
    } else {
        return Ok(());
    }
    let mut atomic: AtomicPointer = AtomicPointer::default();
    let fr_btc = AlkaneId { block: 32, tx: 0 };
    let parcel2 = MessageContextParcel {
        atomic: atomic.derive(&IndexPointer::default()),
        runes: vec![],
        transaction: Transaction {
            version: bitcoin::blockdata::transaction::Version::ONE,
            input: vec![],
            output: vec![],
            lock_time: bitcoin::absolute::LockTime::ZERO,
        },
        block: std::sync::Arc::new(block.clone()),
        height: genesis::GENESIS_BLOCK,
        pointer: 0,
        refund_pointer: 0,
        calldata: (Cellpack {
            target: fr_btc.clone(),
            inputs: vec![0],
        })
        .encipher(),
        sheets: Box::<BalanceSheet<AtomicPointer>>::new(BalanceSheet::default()),
        txindex: 0,
        vout: 0,
        runtime_balances: Box::<BalanceSheet<AtomicPointer>>::new(BalanceSheet::default()),
    };
    let (response3, _gas_used3) = (match simulate_parcel(&parcel2, u64::MAX) {
        Ok((a, b)) => Ok((a, b)),
        Err(e) => {
            println!("{:?}", e);
            Err(e)
        }
    })?;
    pipe_storagemap_to(
        &response3.storage,
        &mut atomic.derive(&IndexPointer::from_keyword("/alkanes/").select(&fr_btc.clone().into())),
    );
    atomic.commit();
    Ok(())
}

pub fn setup_diesel(block: &Block) -> Result<()> {
    // Byte presence at /alkanes/2:0 is the deployed-detection marker (kept
    // for backwards compatibility with already-synced DBs). Execution never
    // reads these bytes — `get_alkane_binary` resolves `2:0` from the static
    // in-binary version map via `precompiled_alkane_wasm_for_height`.
    let mut ptr =
        IndexPointer::from_keyword("/alkanes/").select(&(AlkaneId { block: 2, tx: 0 }).into());
    if ptr.get().len() == 0 {
        ptr.set(Arc::new(compress(genesis_alkane_bytes())?));
    } else {
        return Ok(());
    }
    let mut atomic: AtomicPointer = AtomicPointer::default();
    let myself = AlkaneId { block: 2, tx: 0 };
    let parcel = MessageContextParcel {
        atomic: atomic.derive(&IndexPointer::default()),
        runes: vec![],
        transaction: Transaction {
            version: bitcoin::blockdata::transaction::Version::ONE,
            input: vec![],
            output: vec![],
            lock_time: bitcoin::absolute::LockTime::ZERO,
        },
        block: std::sync::Arc::new(block.clone()),
        height: genesis::GENESIS_BLOCK,
        pointer: 0,
        refund_pointer: 0,
        calldata: (Cellpack {
            target: myself.clone(),
            inputs: vec![0],
        })
        .encipher(),
        sheets: Box::<BalanceSheet<AtomicPointer>>::new(BalanceSheet::default()),
        txindex: 0,
        vout: 0,
        runtime_balances: Box::<BalanceSheet<AtomicPointer>>::new(BalanceSheet::default()),
    };
    let (response, _gas_used) = (match simulate_parcel(&parcel, u64::MAX) {
        Ok((a, b)) => Ok((a, b)),
        Err(e) => {
            println!("{:?}", e);
            Err(e)
        }
    })?;
    let outpoint_bytes = outpoint_encode(&OutPoint {
        txid: tx_hex_to_txid(genesis::GENESIS_OUTPOINT)?,
        vout: 0,
    })?;
    <AlkaneTransferParcel as TryInto<BalanceSheet<AtomicPointer>>>::try_into(
        response.alkanes.into(),
    )?
    .save(
        &mut atomic.derive(
            &RuneTable::for_protocol(AlkaneMessageContext::protocol_tag())
                .OUTPOINT_TO_RUNES
                .select(&outpoint_bytes),
        ),
        false,
    );
    pipe_storagemap_to(
        &response.storage,
        &mut atomic.derive(&IndexPointer::from_keyword("/alkanes/").select(&myself.clone().into())),
    );
    atomic.commit();
    Ok(())
}

pub fn genesis() -> Result<()> {
    let mut atomic: AtomicPointer = AtomicPointer::default();
    sequence_pointer(&atomic).set_value::<u128>(1);
    let outpoint_bytes = outpoint_encode(&OutPoint {
        txid: tx_hex_to_txid(genesis::GENESIS_OUTPOINT)?,
        vout: 0,
    })?;
    atomic
        .derive(&RUNES.OUTPOINT_TO_HEIGHT.select(&outpoint_bytes))
        .set_value(genesis::GENESIS_OUTPOINT_BLOCK_HEIGHT);
    atomic
        .derive(
            &RUNES
                .HEIGHT_TO_TRANSACTION_IDS
                .select_value::<u64>(genesis::GENESIS_OUTPOINT_BLOCK_HEIGHT),
        )
        .append(Arc::new(
            hex::decode(genesis::GENESIS_OUTPOINT)?
                .iter()
                .cloned()
                .rev()
                .collect::<Vec<u8>>(),
        ));
    atomic.commit();
    Ok(())
}
