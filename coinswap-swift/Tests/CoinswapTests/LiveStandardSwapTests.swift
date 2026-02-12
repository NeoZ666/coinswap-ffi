import Foundation
import XCTest
import Coinswap

final class LiveStandardSwapTests: XCTestCase {
    func testLiveTakerFlow() throws {
        try requireLiveTestsEnabled()
        let config = try LiveTestConfig()

        let taker = try Taker.`init`(
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

        let address = try taker.getNextExternalAddress(addressType: AddressType(addrType: "P2WPKH"))
        try fundAddress(address.address, config: config)
        try taker.syncAndSave()
        let updatedBalances = try taker.getBalances()
        XCTAssertGreaterThanOrEqual(updatedBalances.spendable, 0)

        if config.performSwap {
            let params = SwapParams(sendAmount: config.swapAmount, makerCount: 2, manuallySelectedOutpoints: nil)
            let report = try taker.doCoinswap(swapParams: params)
            if let report = report {
                XCTAssertGreaterThan(report.targetAmount, 0)
            }
        }
    }
}
