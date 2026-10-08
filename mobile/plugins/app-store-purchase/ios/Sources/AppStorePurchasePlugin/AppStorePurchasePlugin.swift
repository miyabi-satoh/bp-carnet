import Capacitor
import Foundation
import StoreKit

/// アプリ内課金 (StoreKit 2) で OCR 枠を買う (docs/payments.md)。
/// 枠を足すかはサーバーが Apple から取引を取り直して決めるので、ここは取引 ID と環境を渡すだけにする。
/// 取引を終える (`finish`) のは、サーバーの答えを見た JS が決める。
@objc(AppStorePurchasePlugin)
public class AppStorePurchasePlugin: CAPPlugin, CAPBridgedPlugin {
    public let identifier = "AppStorePurchasePlugin"
    public let jsName = "AppStorePurchase"
    public let pluginMethods: [CAPPluginMethod] = [
        CAPPluginMethod(name: "product", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "purchase", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "unfinishedTransactions", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "finish", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "debugRefundAvailable", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "debugRefundRequest", returnType: CAPPluginReturnPromise)
    ]

    /// JS に渡したが、まだ終えていない取引。`finish` で ID から引く。
    @MainActor private var pending: [UInt64: Transaction] = [:]
    private var updates: Task<Void, Never>?

    override public func load() {
        // 起動中に届く取引 (承認待ちが承認された・ほかの端末で買った等)。JS がまだ聞いていなくても落とさない。
        // Capacitor が聞き手の付く前の知らせを積む先はスレッドセーフでないので、メインスレッドで知らせる。
        updates = Task { @MainActor [weak self] in
            for await result in Transaction.updates {
                guard let self else { return }
                let transaction = self.keep(result)
                self.notifyListeners("transaction", data: Self.describe(transaction), retainUntilConsumed: true)
            }
        }
    }

    deinit {
        updates?.cancel()
    }

    /// 商品の表示用の価格 (`displayPrice`)。見つからなければ `NOT_FOUND` で失敗させる。
    @objc func product(_ call: CAPPluginCall) {
        guard let productId = call.getString("productId") else {
            call.reject("productId が要ります")
            return
        }
        Task {
            do {
                guard let product = try await Product.products(for: [productId]).first else {
                    call.reject("商品が見つかりません", "NOT_FOUND")
                    return
                }
                call.resolve(["displayPrice": product.displayPrice])
            } catch {
                call.reject(error.localizedDescription, nil, error)
            }
        }
    }

    /// 購入シートを出す。結果は `purchased` (取引 ID と環境を添える)・`pending` (承認待ち)・`cancelled`。
    /// `appAccountToken` はサーバーから受け取ったアカウントの UUID。
    @objc func purchase(_ call: CAPPluginCall) {
        guard let productId = call.getString("productId"),
            let token = call.getString("appAccountToken").flatMap(UUID.init(uuidString:))
        else {
            call.reject("productId と appAccountToken (UUID) が要ります")
            return
        }
        Task {
            do {
                guard let product = try await Product.products(for: [productId]).first else {
                    call.reject("商品が見つかりません", "NOT_FOUND")
                    return
                }
                switch try await product.purchase(options: [.appAccountToken(token)]) {
                case .success(let result):
                    let transaction = await keep(result)
                    #if DEBUG
                        UserDefaults.standard.set(String(transaction.id), forKey: Self.lastPurchaseKey)
                    #endif
                    call.resolve(["result": "purchased"].merging(Self.describe(transaction)) { $1 })
                case .pending:
                    call.resolve(["result": "pending"])
                case .userCancelled:
                    call.resolve(["result": "cancelled"])
                @unknown default:
                    call.resolve(["result": "cancelled"])
                }
            } catch {
                call.reject(error.localizedDescription, nil, error)
            }
        }
    }

    /// 終えていない取引 (前の起動でサーバーに送れなかったもの等)。
    @objc func unfinishedTransactions(_ call: CAPPluginCall) {
        Task {
            var transactions: [[String: Any]] = []
            for await result in Transaction.unfinished {
                transactions.append(Self.describe(await keep(result)))
            }
            call.resolve(["transactions": transactions])
        }
    }

    /// 取引を終える。この起動で JS に渡していない取引は何もしない。
    @objc func finish(_ call: CAPPluginCall) {
        guard let id = call.getString("transactionId").flatMap(UInt64.init) else {
            call.reject("transactionId が要ります")
            return
        }
        Task { @MainActor in
            if let transaction = pending.removeValue(forKey: id) {
                await transaction.finish()
            }
            call.resolve()
        }
    }

    /// 開発用の返金の申し出 (`debugRefundRequest`) を使えるか。Debug ビルドだけ `true`。
    @objc func debugRefundAvailable(_ call: CAPPluginCall) {
        #if DEBUG
            call.resolve(["available": true])
        #else
            call.resolve(["available": false])
        #endif
    }

    /// 開発用 (Debug ビルドだけ)。この端末で最後に買った取引で、返金の申し出のシートを出す。
    /// Sandbox で返金を起こし、サーバーに届く `REFUND` の通知を確かめるため
    /// (Apple「Testing refund requests」。Sandbox では申し出が自動で承認される)。
    /// 終えた消耗型は `Transaction.all` に残らないので、買ったときに取引 ID を控えておく。
    @objc func debugRefundRequest(_ call: CAPPluginCall) {
        #if DEBUG
            Task { @MainActor in
                guard let id = UserDefaults.standard.string(forKey: Self.lastPurchaseKey).flatMap(UInt64.init) else {
                    call.reject("この端末で買った取引がありません", "NOT_FOUND")
                    return
                }
                guard let scene = bridge?.viewController?.view.window?.windowScene else {
                    call.reject("画面が見つかりません")
                    return
                }
                do {
                    let status = try await Transaction.beginRefundRequest(for: id, in: scene)
                    call.resolve(["status": status == .success ? "success" : "cancelled"])
                } catch {
                    call.reject(error.localizedDescription, nil, error)
                }
            }
        #else
            call.reject("Debug ビルドだけで使えます", "UNAVAILABLE")
        #endif
    }

    #if DEBUG
        private static let lastPurchaseKey = "AppStorePurchase.debugLastPurchase"
    #endif

    /// 署名を確かめられなかった取引も渡す。足すかはサーバーが Apple から取り直して決めるため。
    @MainActor private func keep(_ result: VerificationResult<Transaction>) -> Transaction {
        let transaction = result.unsafePayloadValue
        pending[transaction.id] = transaction
        return transaction
    }

    private static func describe(_ transaction: Transaction) -> [String: Any] {
        // environment は `Production`・`Sandbox`・`Xcode` (StoreKit の設定ファイルでのテスト)。
        ["transactionId": String(transaction.id), "environment": transaction.environment.rawValue]
    }
}
