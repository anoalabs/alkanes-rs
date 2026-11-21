//! Validates the `regtest_frsigil` build (B8's regtest chain):
//!   - the frSIGIL (32:1), cFIRE-SIGIL (4:866) and SUBMINER-AUTH (2:96775)
//!     capability units are premined onto block 1's coinbase, a
//!     b8-controllable, bitcoin-spendable regtest outpoint;
//!   - mainnet's DIESEL v3 governance set is live at its mainnet ids with the
//!     mainnet init args (gate 4:1001 at 192 bp into treasury 2:96782, whose
//!     payee is 2:96775), DIESEL runs v3 from genesis with one-per-tx mints;
//!   - spending the coinbase authorizes frBTC `set_signer` (the frSIGIL check
//!     stays enforced) and forwards every sibling sigil back;
//!   - a mint is split by the mainnet gate; the SUBMINER-AUTH holder collects
//!     the treasury; the cFIRE-SIGIL holder re-splits the gate, clears it and
//!     collects DIESEL's claimable fees;
//!   - block fuel is metered like mainnet (proportional to block space).

use crate::indexer::index_block;
use crate::network::regtest_subminer::{
    CALLER_BP, CFIRE_SIGIL, DIESEL, SUBMINER_AUTH, SUBMINER_GATE, SUBMINER_TREASURY,
};
use crate::tests::helpers::{
    self as alkane_helpers, assert_return_context, assert_revert_context, clear,
    get_sheet_for_outpoint,
};
use crate::vm::fuel::{minimum_fuel, FuelTank};
use alkanes_support::cellpack::Cellpack;
use alkanes_support::id::AlkaneId;
use anyhow::Result;
use bitcoin::blockdata::transaction::{OutPoint, Version};
use bitcoin::{Amount, Block, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
use metashrew_core::index_pointer::IndexPointer;
use metashrew_support::index_pointer::KeyValuePointer;
use ordinals::Runestone;
use protorune::protostone::Protostones;
use protorune::test_helpers::{
    create_block_with_coinbase_tx, create_coinbase_transaction, get_address, ADDRESS1,
};
use protorune_support::balance_sheet::{BalanceSheetOperations, ProtoruneRuneId};
use protorune_support::protostone::{Protostone, ProtostoneEdict};
use wasm_bindgen_test::wasm_bindgen_test;

const FRSIGIL: AlkaneId = AlkaneId { block: 32, tx: 1 };
const FRBTC: AlkaneId = AlkaneId { block: 32, tx: 0 };
const BP: u128 = 10_000;
/// Regtest block reward below the first halving.
const REWARD: u128 = 5_000_000_000;

/// What DIESEL v3 pays per mint in a block of `mints` mints at `height`, and
/// what it accrues to claimable fees for that block (`diesel_fee` plus the
/// division remainder). The test coinbase carries a fixed total, so
/// `diesel_fee = min(reward / 2, coinbase_total - reward)`.
fn expect(height: u32, mints: u128) -> (u128, u128) {
    let total: u128 = create_coinbase_transaction(height)
        .output
        .iter()
        .map(|o| o.value.to_sat() as u128)
        .sum();
    let diesel_fee = std::cmp::min(REWARD / 2, total.saturating_sub(REWARD));
    let value = (REWARD - diesel_fee) / mints;
    (value, diesel_fee + (REWARD - diesel_fee) % mints)
}

fn rune(id: AlkaneId) -> ProtoruneRuneId {
    ProtoruneRuneId { block: id.block, tx: id.tx }
}

fn storage(id: AlkaneId, key: &str) -> IndexPointer {
    IndexPointer::default()
        .keyword("/alkanes/")
        .select(&id.into())
        .keyword("/storage/")
        .select(&key.as_bytes().to_vec())
}

fn read_u128(id: AlkaneId, key: &str) -> u128 {
    storage(id, key).get_value::<u128>()
}

fn read_id(id: AlkaneId, key: &str) -> AlkaneId {
    AlkaneId::try_from(storage(id, key).get().as_ref().clone()).expect("alkane id bytes")
}

fn coinbase_outpoint(height: u32) -> OutPoint {
    OutPoint::new(create_coinbase_transaction(height).compute_txid(), 0)
}

/// Index block 1: genesis runs (GENESIS_BLOCK = 0), DIESEL v3, frBTC and
/// frSIGIL deploy, and the deferred hook premines the three capability units
/// onto this coinbase and installs the SUBMINER set. Returns that coinbase.
fn boot() -> Result<OutPoint> {
    clear();
    let block1 = create_block_with_coinbase_tx(1);
    let coinbase = OutPoint {
        txid: block1.txdata[0].compute_txid(),
        vout: 0,
    };
    index_block(&block1, 1)?;
    Ok(coinbase)
}

/// One tx calling `target` with `inputs`, spending `input`, in its own block
/// at `height`. Two real outputs + the OP_RETURN, so the message's shadow
/// vout is 3 and the response lands on vout 0.
fn call(height: u32, target: AlkaneId, inputs: Vec<u128>, input: OutPoint) -> Result<(Block, Transaction)> {
    let mut block = create_block_with_coinbase_tx(height);
    let tx = alkane_helpers::create_multiple_cellpack_with_witness_and_in(
        Witness::new(),
        vec![Cellpack { target, inputs }],
        input,
        false,
    );
    block.txdata.push(tx.clone());
    index_block(&block, height)?;
    Ok((block, tx))
}

fn ok(tx: &Transaction) -> Result<()> {
    assert_return_context(&OutPoint { txid: tx.compute_txid(), vout: 3 }, |_| Ok(()))
}

fn reverted(tx: &Transaction, needle: &str) -> Result<()> {
    assert_revert_context(&OutPoint { txid: tx.compute_txid(), vout: 3 }, needle)
}

fn at(block: &Block, tx: usize, vout: u32, id: AlkaneId) -> Result<u128> {
    Ok(get_sheet_for_outpoint(block, tx, vout)?.get(&rune(id)))
}

/// A plain transfer: one edict-only protostone moving `amount` of `id` from
/// `input` to output 1, everything else by pointer to output 0. Used to put a
/// single capability unit on its own outpoint.
fn split(height: u32, input: OutPoint, id: AlkaneId, amount: u128) -> Result<(Block, Transaction)> {
    let spk = get_address(&ADDRESS1()).script_pubkey();
    let tx = alkane_helpers::create_protostone_tx_with_inputs_and_default_pointer(
        vec![TxIn {
            previous_output: input,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        vec![
            TxOut { value: Amount::from_sat(546), script_pubkey: spk.clone() },
            TxOut { value: Amount::from_sat(546), script_pubkey: spk },
        ],
        Protostone {
            burn: None,
            message: vec![],
            edicts: vec![ProtostoneEdict { id: rune(id), amount, output: 1 }],
            refund: Some(0),
            pointer: Some(0),
            from: None,
            protocol_tag: 1,
        },
        0,
    );
    let mut block = create_block_with_coinbase_tx(height);
    block.txdata.push(tx.clone());
    index_block(&block, height)?;
    Ok((block, tx))
}

/// A set_signer tx spending `input` (which must carry frSIGIL): output 0 is
/// where the sigils are routed back (runestone + protostone pointer 0),
/// output 1 is the new signer (signer_vout = 1, distinct from the pointer),
/// output 2 is the runestone OP_RETURN.
fn set_signer_tx(input: OutPoint, signer_vout: u128) -> Transaction {
    let protostone = vec![Protostone {
        message: Cellpack { target: FRBTC, inputs: vec![1, signer_vout] }.encipher(),
        pointer: Some(0),
        refund: Some(0),
        edicts: vec![],
        from: None,
        burn: None,
        protocol_tag: 1,
    }];
    let runestone: ScriptBuf = (Runestone {
        etching: None,
        pointer: Some(0),
        edicts: Vec::new(),
        mint: None,
        protocol: protostone.encipher().ok(),
    })
    .encipher();
    let spk = get_address(&ADDRESS1()).script_pubkey();
    Transaction {
        version: Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: input,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::default(),
        }],
        output: vec![
            TxOut { value: Amount::from_sat(546), script_pubkey: spk.clone() },
            TxOut { value: Amount::from_sat(546), script_pubkey: spk },
            TxOut { value: Amount::from_sat(0), script_pubkey: runestone },
        ],
    }
}

// ---------------------------------------------------------------- tests ----

/// Block 1's coinbase holds the three capability units, and mainnet's
/// governance set is installed with the mainnet init args. DIESEL is v3 with
/// one-per-tx mints enabled and the compile-time default gate in effect.
#[wasm_bindgen_test]
fn test_genesis_installs_mainnet_governance_set() -> Result<()> {
    let coinbase = boot()?;
    let block1 = create_block_with_coinbase_tx(1);
    assert_eq!(at(&block1, 0, 0, FRSIGIL)?, 1, "frSIGIL premined to the coinbase");
    assert_eq!(at(&block1, 0, 0, CFIRE_SIGIL)?, 1, "cFIRE-SIGIL premined to the coinbase");
    assert_eq!(at(&block1, 0, 0, SUBMINER_AUTH)?, 1, "SUBMINER-AUTH premined to the coinbase");
    let _ = coinbase;

    assert_eq!(read_u128(SUBMINER_GATE, "/caller-bp"), CALLER_BP);
    assert_eq!(read_id(SUBMINER_GATE, "/treasury"), SUBMINER_TREASURY);
    assert_eq!(read_id(SUBMINER_GATE, "/sigil"), CFIRE_SIGIL);
    assert_eq!(read_id(SUBMINER_GATE, "/diesel"), DIESEL);
    assert_eq!(read_id(SUBMINER_TREASURY, "/payee"), SUBMINER_AUTH);
    assert_eq!(read_id(SUBMINER_TREASURY, "/asset"), DIESEL);
    assert_eq!(storage(DIESEL, "/upgrade_initialized").get_value::<u8>(), 1);
    assert_eq!(storage(DIESEL, "/mint-gate").get().len(), 0, "default gate, as on mainnet");
    Ok(())
}

/// Spending the block-1 coinbase into set_signer is AUTHORIZED, and the two
/// sibling sigils ride along and come back on the pointer output.
#[wasm_bindgen_test]
fn test_coinbase_authorizes_set_signer_and_keeps_sibling_sigils() -> Result<()> {
    let coinbase = boot()?;
    let tx = set_signer_tx(coinbase, 1);
    let mut block2 = create_block_with_coinbase_tx(2);
    block2.txdata.push(tx.clone());
    index_block(&block2, 2)?;
    // shadow vout = tx.output.len() + 1 + protostone index = 3 + 1 + 0
    assert_return_context(&OutPoint { txid: tx.compute_txid(), vout: 4 }, |_| Ok(()))?;
    assert_eq!(at(&block2, 1, 0, FRSIGIL)?, 1);
    assert_eq!(at(&block2, 1, 0, CFIRE_SIGIL)?, 1);
    assert_eq!(at(&block2, 1, 0, SUBMINER_AUTH)?, 1);
    Ok(())
}

/// Control: without frSIGIL, set_signer reverts with the auth error.
#[wasm_bindgen_test]
fn test_set_signer_still_requires_frsigil() -> Result<()> {
    boot()?;
    let (_, tx) = call(2, FRBTC, vec![1, 0], OutPoint::default())?;
    reverted(&tx, "Auth token is not in incoming alkanes")
}

/// A bare `[77]` mint is split by the mainnet gate: the minter keeps 192 bp,
/// the treasury is credited the rest, and the emission ledger (live from
/// genesis) settles the mint-free blocks' rewards into claimable fees.
#[wasm_bindgen_test]
fn test_mint_is_split_by_the_mainnet_gate() -> Result<()> {
    boot()?;
    let (block, tx) = call(2, DIESEL, vec![77], coinbase_outpoint(2))?;
    ok(&tx)?;
    let (mint, fee) = expect(2, 1);
    let share = mint * CALLER_BP / BP;
    assert_eq!(at(&block, 1, 0, DIESEL)?, share, "minter keeps caller_bp");
    assert_eq!(read_u128(SUBMINER_TREASURY, "/deposited"), mint - share);
    assert_eq!(read_u128(SUBMINER_GATE, "/routed-total"), mint);
    assert_eq!(read_u128(SUBMINER_GATE, "/routed-to-treasury"), mint - share);
    // blocks 0 and 1 had no mint: their full rewards plus this block's
    // diesel_fee are claimable fees (the ledger runs from genesis)
    assert_eq!(read_u128(DIESEL, "/fees"), 2 * REWARD + fee);
    Ok(())
}

/// A second mint in the same block halves the per-mint value: mints are
/// one-per-tx (the `/upgrade_initialized` preset), as on mainnet.
#[wasm_bindgen_test]
fn test_mints_are_one_per_tx() -> Result<()> {
    boot()?;
    let mut block = create_block_with_coinbase_tx(2);
    let coinbase = create_coinbase_transaction(2).compute_txid();
    for i in 0..2u32 {
        block.txdata.push(alkane_helpers::create_multiple_cellpack_with_witness_and_in(
            Witness::new(),
            vec![Cellpack { target: DIESEL, inputs: vec![77] }],
            OutPoint::new(coinbase, i),
            false,
        ));
    }
    index_block(&block, 2)?;
    ok(&block.txdata[1])?;
    ok(&block.txdata[2])?;
    let (per_mint, _) = expect(2, 2);
    assert_eq!(per_mint, expect(2, 1).0 / 2, "two mints halve the per-mint value");
    assert_eq!(at(&block, 1, 0, DIESEL)?, per_mint * CALLER_BP / BP);
    assert_eq!(at(&block, 2, 0, DIESEL)?, per_mint * CALLER_BP / BP);
    Ok(())
}

/// The SUBMINER-AUTH holder collects everything the treasury holds; the
/// payee token comes back with the payout.
#[wasm_bindgen_test]
fn test_subminer_auth_collects_the_treasury() -> Result<()> {
    let coinbase = boot()?;
    let (_, mint_tx) = call(2, DIESEL, vec![77], coinbase_outpoint(2))?;
    ok(&mint_tx)?;
    let (mint, _) = expect(2, 1);
    let held = mint - mint * CALLER_BP / BP;
    assert_eq!(read_u128(SUBMINER_TREASURY, "/deposited"), held);

    // isolate the payee token: collect wants the sigil "and nothing else"
    let (split_block, split_tx) = split(3, coinbase, SUBMINER_AUTH, 1)?;
    assert_eq!(at(&split_block, 1, 1, SUBMINER_AUTH)?, 1);
    assert_eq!(at(&split_block, 1, 0, FRSIGIL)?, 1);
    assert_eq!(at(&split_block, 1, 0, CFIRE_SIGIL)?, 1);

    let auth = OutPoint { txid: split_tx.compute_txid(), vout: 1 };
    let (block, collect) = call(4, SUBMINER_TREASURY, vec![2], auth)?;
    ok(&collect)?;
    assert_eq!(at(&block, 1, 0, DIESEL)?, held, "the whole treasury balance is paid out");
    assert_eq!(at(&block, 1, 0, SUBMINER_AUTH)?, 1, "the payee token is handed back");

    // control: the frSIGIL + cFIRE-SIGIL outpoint does not authorize collect
    let others = OutPoint { txid: split_tx.compute_txid(), vout: 0 };
    let (_, bad) = call(5, SUBMINER_TREASURY, vec![2], others)?;
    reverted(&bad, "payee sigil")
}

/// The cFIRE-SIGIL holder governs the policy: re-split the gate, clear the
/// gate (mints then accrue to DIESEL's claimable fees), collect those fees.
#[wasm_bindgen_test]
fn test_cfire_sigil_governs_gate_and_fees() -> Result<()> {
    let coinbase = boot()?;
    let (_, split_tx) = split(2, coinbase, CFIRE_SIGIL, 1)?;
    let mut sigil = OutPoint { txid: split_tx.compute_txid(), vout: 1 };

    // set_split(10000): minters keep everything
    let (_, tx) = call(3, SUBMINER_GATE, vec![2, BP], sigil)?;
    ok(&tx)?;
    sigil = OutPoint { txid: tx.compute_txid(), vout: 0 };
    assert_eq!(read_u128(SUBMINER_GATE, "/caller-bp"), BP);
    let (block, mint_tx) = call(4, DIESEL, vec![77], coinbase_outpoint(4))?;
    ok(&mint_tx)?;
    assert_eq!(at(&block, 1, 0, DIESEL)?, expect(4, 1).0, "100% to the minter");

    // set_mint_gate(0:0): no gate, mints accrue to claimable fees
    let fees_before = read_u128(DIESEL, "/fees");
    let (_, tx) = call(5, DIESEL, vec![80, 0, 0], sigil)?;
    ok(&tx)?;
    sigil = OutPoint { txid: tx.compute_txid(), vout: 0 };
    let (block, mint_tx) = call(6, DIESEL, vec![77], coinbase_outpoint(6))?;
    ok(&mint_tx)?;
    assert_eq!(at(&block, 1, 0, DIESEL)?, 0, "nothing to the minter without a gate");
    // block 5 had no mint (its full reward) + this block's whole mint + its
    // diesel_fee: with no gate the block still emits exactly its reward
    let (mint, fee) = expect(6, 1);
    assert_eq!(read_u128(DIESEL, "/fees"), fees_before + REWARD + mint + fee);

    // collect_fees pays the whole claimable balance to the sigil holder
    let claimable = read_u128(DIESEL, "/fees");
    let (block, tx) = call(7, DIESEL, vec![78], sigil)?;
    ok(&tx)?;
    assert_eq!(at(&block, 1, 0, DIESEL)?, claimable);
    assert_eq!(at(&block, 1, 0, CFIRE_SIGIL)?, 1, "the sigil is handed back");
    assert_eq!(read_u128(DIESEL, "/fees"), 0);

    // control: governance without the sigil is refused
    let (_, bad) = call(8, SUBMINER_GATE, vec![2, 500], coinbase_outpoint(8))?;
    reverted(&bad, "cFIRE-SIGIL not present")?;
    let (_, bad) = call(9, DIESEL, vec![80, 0, 0], coinbase_outpoint(9))?;
    reverted(&bad, "cFIRE-SIGIL not present")
}

/// Fuel is metered like mainnet: a tx's allotment is its share of the block's
/// alkane bytes, never below the per-tx minimum (the stock regtest build
/// hands every tx exactly the minimum).
#[wasm_bindgen_test]
fn test_block_fuel_is_metered_by_block_space() -> Result<()> {
    boot()?;
    let mut block = create_block_with_coinbase_tx(2);
    block.txdata.push(alkane_helpers::create_multiple_cellpack_with_witness_and_in(
        Witness::new(),
        vec![Cellpack { target: DIESEL, inputs: vec![77] }],
        coinbase_outpoint(2),
        false,
    ));
    FuelTank::initialize(&block, 2);
    let tank = FuelTank::get_fuel_tank_copy().expect("tank");
    assert!(tank.size > 0, "a mint tx counts toward the block's alkane bytes");
    // a tx taking half the block's alkane bytes gets half the block fuel
    FuelTank::fuel_transaction(tank.size / 2, 1, 2);
    let allotted = FuelTank::start_fuel();
    assert!(allotted > minimum_fuel(2), "allotted {allotted} must exceed the minimum");
    assert_eq!(allotted, tank.block_fuel * (tank.size / 2) / tank.size);
    // the allotment is always at least the per-tx minimum
    FuelTank::initialize(&block, 2);
    FuelTank::fuel_transaction(1, 1, 2);
    let metered = tank.block_fuel / tank.size;
    assert_eq!(FuelTank::start_fuel(), std::cmp::max(minimum_fuel(2), metered));
    Ok(())
}
