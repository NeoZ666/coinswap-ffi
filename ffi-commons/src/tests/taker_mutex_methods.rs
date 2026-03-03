//! Mutex blocking behavior tests for Coinswap Taker UniFFI bindings.

use crate::{
    taker::{SwapParams, Taker},
    tests::docker_helpers::{self, DockerBitcoind},
};
use bitcoin::Amount;
use bitcoind::bitcoincore_rpc::RpcApi;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn setup_bitcoind_and_taker(wallet_name: &str) -> (std::sync::Arc<Taker>, DockerBitcoind) {
    let bitcoind = DockerBitcoind::connect().expect("Failed to connect to Docker bitcoind");

    let rpc_config = docker_helpers::get_docker_rpc_config(wallet_name);

    let taker = Taker::init(
        None,
        Some(wallet_name.to_string()),
        Some(rpc_config),
        Some(9051),
        Some("coinswap".to_string()),
        docker_helpers::DOCKER_BITCOIN_ZMQ.to_string(),
        None,
    )
    .unwrap();

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
    coinswap::utill::setup_taker_logger(log::LevelFilter::Info, true, None);

    let wallet_name = "test-ffi-taker-mutex";
    cleanup_wallet(wallet_name);

    let (taker, bitcoind) = setup_bitcoind_and_taker(wallet_name);

    let funding_address = taker
        .get_next_external_address(crate::AddressType {
            addr_type: "P2WPKH".to_string(),
        })
        .expect("Failed to get funding address")
        .address
        .parse::<bitcoin::Address<bitcoin::address::NetworkUnchecked>>()
        .expect("Invalid funding address")
        .require_network(bitcoin::Network::Regtest)
        .expect("Funding address is not regtest");

    bitcoind
        .send_to_address_from_funding_wallet(&funding_address, Amount::from_sat(120_000))
        .expect("Failed to fund taker from test wallet");

    taker.sync_and_save().expect("Failed to sync funded wallet");
    let balances = taker.get_balances().expect("Failed to get balances");
    assert!(
        balances.spendable >= 120_000,
        "Expected funded spendable balance, got {}",
        balances.spendable
    );

    println!(
        "Waiting for offerbook synchronization to complete…{:?}",
        taker.is_offerbook_syncing()
    );
    for _ in 1..=2 {
        println!("sync now {:?}", taker.run_offer_sync_now());
        println!(
            "Waiting for offerbook synchronization to complete…{:?}",
            taker.is_offerbook_syncing()
        );
        std::thread::sleep(Duration::from_secs(15));
    }

    let (started_tx, started_rx) = mpsc::channel();

    let swapper = std::sync::Arc::clone(&taker);
    let swap_thread = std::thread::spawn(move || {
        started_tx.send(()).expect("Failed to signal swap start");
        swapper.do_coinswap(SwapParams {
            send_amount: 50_000,
            maker_count: 2,
            manually_selected_outpoints: None,
        })
    });

    started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("Swap thread did not start in time");

    std::thread::sleep(Duration::from_millis(150));

    let reader = std::sync::Arc::clone(&taker);
    let start = Instant::now();
    let reader_thread = std::thread::spawn(move || {
        let result = reader.get_next_external_address(crate::AddressType {
            addr_type: "P2WPKH".to_string(),
        });
        let elapsed = start.elapsed();
        (result, elapsed)
    });

    let swap_result = swap_thread.join().expect("Swap thread panicked");
    let (address_result, elapsed) = reader_thread.join().expect("Reader thread panicked");

    let swap_failed_fast_due_to_offers = matches!(&swap_result, Err(e) if format!("{:?}", e).contains("NotEnoughMakersInOfferBook") || e.to_string().contains("NotEnoughMakersInOfferBook"));

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
