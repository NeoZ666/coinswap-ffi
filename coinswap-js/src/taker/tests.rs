use crate::types::{Amount, FidelityBond, LockTime, OutPoint, PublicKey};
use coinswap::bitcoin::absolute::LockTime as csLockTime;
use coinswap::bitcoind::bitcoincore_rpc::{Auth, Client, RpcApi};
use std::process::Command;
use std::sync::Arc;

#[test]
fn test_locktime_conversion_basic() {
  let block_locktime = csLockTime::from_height(500000).unwrap();
  let napi_block = LockTime::from(block_locktime);

  let time_locktime = csLockTime::from_time(1234567890).unwrap();
  let napi_time = LockTime::from(time_locktime);

  println!("From Rust -> Javascript : ");
  println!("Block locktime: {:?} -> {:?}", block_locktime, napi_block);
  println!("Time locktime: {:?} -> {:?}", time_locktime, napi_time);
}

#[test]
fn test_fidelity_bond_creation() {
  // Create a mock fidelity bond to see the structure
  let bond = FidelityBond {
    outpoint: OutPoint {
      txid: "abc123def456789".to_string(),
      vout: 0,
    },
    amount: Amount { sats: 100000 },
    lock_time: LockTime {
      lock_type: "Blocks".to_string(),
      value: 750000,
    },
    pubkey: PublicKey {
      compressed: true,
      inner: vec![2, 123, 45, 67, 89],
    },
    conf_height: Some(500000),
    cert_expiry: Some(144),
    is_spent: false,
  };

  println!("FidelityBond structure:");
  println!("  outpoint: {}:{}", bond.outpoint.txid, bond.outpoint.vout);
  println!("  amount: {} sats", bond.amount.sats);
  println!("  lock_time: {:?}", bond.lock_time);
  println!("  pubkey compressed: {}", bond.pubkey.compressed);
  println!("  pubkey bytes: {:?}", bond.pubkey.inner);
  println!("  conf_height: {:?}", bond.conf_height);
  println!("  cert_expiry: {:?}", bond.cert_expiry);
  println!("  is_spent: {}", bond.is_spent);
}

const DOCKER_BITCOIN_RPC_URL: &str = "http://localhost:18442";
const DOCKER_BITCOIN_RPC_USER: &str = "user";
const DOCKER_BITCOIN_RPC_PASS: &str = "password";
const DOCKER_BITCOIN_ZMQ: &str = "tcp://127.0.0.1:28332";

struct DockerBitcoind {
  client: Client,
}

impl DockerBitcoind {
  fn connect() -> Result<Self, String> {
    let client = Client::new(
      DOCKER_BITCOIN_RPC_URL,
      Auth::UserPass(
        DOCKER_BITCOIN_RPC_USER.to_string(),
        DOCKER_BITCOIN_RPC_PASS.to_string(),
      ),
    )
    .map_err(|e| format!("Failed to connect to Docker bitcoind: {}", e))?;

    client
      .get_blockchain_info()
      .map_err(|e| format!("Failed to get blockchain info: {}", e))?;

    Ok(Self { client })
  }

  fn send_to_address_from_funding_wallet(
    &self,
    address: &coinswap::bitcoin::Address,
    amount: coinswap::bitcoin::Amount,
  ) -> Result<coinswap::bitcoin::Txid, String> {
    let test_wallet_url = format!("{}/wallet/{}", DOCKER_BITCOIN_RPC_URL, "test");
    let test_client = Client::new(
      &test_wallet_url,
      Auth::UserPass(
        DOCKER_BITCOIN_RPC_USER.to_string(),
        DOCKER_BITCOIN_RPC_PASS.to_string(),
      ),
    )
    .map_err(|e| format!("Failed to connect to test wallet: {}", e))?;

    let txid = test_client
      .send_to_address(address, amount, None, None, None, None, None, None)
      .map_err(|e| format!("Failed to send to address from test wallet: {}", e))?;

    let mining_address = test_client
      .get_new_address(None, None)
      .map_err(|e| format!("Failed to get new address: {}", e))?
      .require_network(coinswap::bitcoin::Network::Regtest)
      .map_err(|e| format!("Failed to require network: {}", e))?;

    test_client
      .generate_to_address(1, &mining_address)
      .map_err(|e| format!("Failed to generate blocks: {}", e))?;

    println!("Sent {} sats to {}, txid: {}", amount, address, txid);
    Ok(txid)
  }
}

fn get_docker_rpc_config(wallet_name: &str) -> crate::types::RPCConfig {
  crate::types::RPCConfig {
    url: "localhost:18442".to_string(),
    username: DOCKER_BITCOIN_RPC_USER.to_string(),
    password: DOCKER_BITCOIN_RPC_PASS.to_string(),
    wallet_name: wallet_name.to_string(),
  }
}

fn setup_bitcoind_and_taker(wallet_name: &str) -> (super::Taker, DockerBitcoind) {
  let bitcoind = DockerBitcoind::connect().expect("Failed to connect to Docker bitcoind");

  let rpc_config = get_docker_rpc_config(wallet_name);

  let taker = super::Taker::init(
    None,
    Some(wallet_name.to_string()),
    Some(rpc_config),
    Some(9051),
    Some("coinswap".to_string()),
    DOCKER_BITCOIN_ZMQ.to_string(),
    None,
  )
  .unwrap();

  println!("Initialized Taker with Docker bitcoind RPC wallet: {}", wallet_name);

  (taker, bitcoind)
}

fn cleanup_wallet(wallet_name: &str) {
  use std::fs;
  use std::path::PathBuf;

  let mut wallet_dir = PathBuf::from(env!("HOME"));
  wallet_dir.push(".coinswap");

  if wallet_dir.exists() {
    let _ = fs::remove_dir_all(&wallet_dir);
  }

  if let Ok(bitcoind) = DockerBitcoind::connect() {
    let _ = bitcoind.client.unload_wallet(Some(wallet_name));
  }

  let _ = Command::new("docker")
    .args([
      "exec",
      "coinswap-ffi-bitcoind",
      "rm",
      "-rf",
      &format!("/home/bitcoin/.bitcoin/wallets/{}", wallet_name),
    ])
    .output();
}

#[test]
fn test_mutex_blocks_concurrent_access_with_docker_setup() {
  use coinswap::bitcoin::Amount as BtcAmount;
  use std::sync::mpsc;
  use std::thread;
  use std::time::{Duration, Instant};

  coinswap::utill::setup_taker_logger(log::LevelFilter::Info, true, None);

  let wallet_name = "test-js-taker-mutex";
  cleanup_wallet(wallet_name);

  let (mut taker, bitcoind) = setup_bitcoind_and_taker(wallet_name);

  let funding_address = taker
    .get_next_external_address(crate::types::AddressType::P2WPKH)
    .expect("Failed to get funding address")
    .address
    .parse::<coinswap::bitcoin::Address<coinswap::bitcoin::address::NetworkUnchecked>>()
    .expect("Invalid funding address")
    .require_network(coinswap::bitcoin::Network::Regtest)
    .expect("Funding address is not regtest");

  bitcoind
    .send_to_address_from_funding_wallet(&funding_address, BtcAmount::from_sat(120_000))
    .expect("Failed to fund taker from test wallet");

  taker.sync_and_save().expect("Failed to sync funded wallet");
  let balances = taker.get_balances().expect("Failed to get balances");
  assert!(
    balances.spendable >= 120_000,
    "Expected funded spendable balance, got {}",
    balances.spendable
  );


  let _ =  taker.is_offerbook_syncing();
  let _ = taker.run_offer_sync_now();
  thread::sleep(Duration::from_secs(45));
  while taker.is_offerbook_syncing().unwrap_or(false) {
    println!(
      "Waiting for offerbook synchronization to complete…"
    );
    thread::sleep(Duration::from_secs(15));
  }

  let taker = Arc::new(taker);

  let (started_tx, started_rx) = mpsc::channel();

  let swapper = Arc::clone(&taker);
  let swap_thread = thread::spawn(move || {
    started_tx.send(()).expect("Failed to signal swap start");
    swapper.do_coinswap(super::SwapParams {
      send_amount: 50_000,
      maker_count: 2,
      manually_selected_outpoints: None,
    })
  });

  started_rx
    .recv_timeout(Duration::from_secs(2))
    .expect("Swap thread did not start in time");

  thread::sleep(Duration::from_millis(150));

  let reader = Arc::clone(&taker);
  let start = Instant::now();
  let reader_thread = thread::spawn(move || {
    let result = reader
      .inner
      .lock()
      .expect("Failed to acquire taker lock")
      .get_wallet_mut()
      .get_next_external_address(coinswap::wallet::AddressType::P2WPKH)
      .map(|addr| addr.to_string());
    let elapsed = start.elapsed();
    (result, elapsed)
  });

  let swap_result = swap_thread.join().expect("Swap thread panicked");
  let (address_result, elapsed) = reader_thread.join().expect("Reader thread panicked");

  let swap_failed_fast_due_to_offers =
    matches!(&swap_result, Err(e) if e.reason.contains("NotEnoughMakersInOfferBook"));

  match swap_result {
    Ok(Some(report)) => {
      println!("Swap completed successfully: {:?}", report.swap_id);
    }
    Ok(None) => {
      println!("Swap completed without report");
    }
    Err(e) => {
      println!("Swap failed (allowed in test env): {:?}", e);
    }
  }

  let min_blocked_time = Duration::from_millis(100);
  if !swap_failed_fast_due_to_offers {
    assert!(
      elapsed >= min_blocked_time,
      "get_next_external_address was not blocked by taker mutex during swap: {:?} < {:?}",
      elapsed,
      min_blocked_time
    );
  } else {
    println!(
      "Skipping strict blocked-time assertion because coinswap exited early with NotEnoughMakersInOfferBook"
    );
  }

  assert!(
    address_result.is_ok(),
    "get_next_external_address failed after lock release: {:?}",
    address_result.err()
  );
}
