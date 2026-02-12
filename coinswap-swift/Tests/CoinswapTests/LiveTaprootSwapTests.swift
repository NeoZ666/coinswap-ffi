import Foundation
import XCTest
import Coinswap

final class LiveTaprootSwapTests: XCTestCase {
    func testLiveTaprootTakerFlow() throws {
        try requireLiveTestsEnabled()
        let config = try LiveTestConfig(walletNameOverride: "swift_taproot_wallet")

        let taker = try TaprootTaker.`init`(
            dataDir: config.dataDir,
            walletFileName: config.walletName,
            rpcConfig: config.rpcConfig,
            controlPort: config.torControlPort,
            torAuthPassword: config.torAuthPassword,
            zmqAddr: config.zmqAddr,
            password: config.walletPassword
        )

        try taker.setupLogging(dataDir: config.dataDir, logLevel: "Info")

        try taker.runOfferSyncNow()
        Thread.sleep(forTimeInterval: 10.0)

        let _ = try taker.getWalletName()
        let balances = try taker.getBalances()
        XCTAssertEqual(balances.spendable, 0)

        let address = try taker.getNextExternalAddress(addressType: AddressType(addrType: "P2TR"))
        try fundAddress(address.address, config: config)
        try taker.syncAndSave()
        let updatedBalances = try taker.getBalances()
        XCTAssertGreaterThanOrEqual(updatedBalances.spendable, 0)

        if config.performSwap {
            let params = TaprootSwapParams(
                sendAmount: config.swapAmount,
                makerCount: 2,
                txCount: nil,
                requiredConfirms: nil,
                manuallySelectedOutpoints: nil
            )
            let report = try taker.doCoinswap(swapParams: params)
            if let report = report {
                XCTAssertGreaterThan(report.targetAmount, 0)
            }
        }
    }
}
