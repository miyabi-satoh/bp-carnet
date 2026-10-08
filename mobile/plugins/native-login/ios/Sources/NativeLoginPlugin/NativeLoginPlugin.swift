import AuthenticationServices
import Capacitor
import Foundation
import GoogleSignIn
import LineSDK

/// 各社の SDK でログインし、サーバーへ渡す値を返す (docs/mobile-app.md)。
/// 既製のプラグインは使わない。Google のものは Facebook の SDK まで一緒に入り、LINE のものは保守されていないため。
@objc(NativeLoginPlugin)
public class NativeLoginPlugin: CAPPlugin, CAPBridgedPlugin {
    public let identifier = "NativeLoginPlugin"
    public let jsName = "NativeLogin"
    public let pluginMethods: [CAPPluginMethod] = [
        CAPPluginMethod(name: "signInWithGoogle", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "signInWithLine", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "signInWithApple", returnType: CAPPluginReturnPromise)
    ]

    override public func load() {
        NotificationCenter.default.addObserver(
            self, selector: #selector(handleOpenUrl(_:)), name: .capacitorOpenURL, object: nil)
    }

    @objc private func handleOpenUrl(_ notification: Notification) {
        guard let object = notification.object as? [String: Any], let url = object["url"] as? URL else { return }
        if GIDSignIn.sharedInstance.handle(url) { return }
        DispatchQueue.main.async {
            _ = LoginManager.shared.application(UIApplication.shared, open: url)
        }
    }

    /// Google Sign-In でサーバー用の認可コード (`serverAuthCode`) を得る。
    /// `clientId` は iOS 用のクライアント、`serverClientId` はサーバー (ウェブ用のクライアント)。
    /// 閉じられたら `CANCELED` で失敗させる。
    @objc func signInWithGoogle(_ call: CAPPluginCall) {
        guard let clientId = call.getString("clientId"), let serverClientId = call.getString("serverClientId") else {
            call.reject("clientId と serverClientId が要ります")
            return
        }
        DispatchQueue.main.async {
            guard let presenting = self.bridge?.viewController else {
                call.reject("画面を出せません")
                return
            }
            let signIn = GIDSignIn.sharedInstance
            signIn.configuration = GIDConfiguration(clientID: clientId, serverClientID: serverClientId)
            signIn.signIn(withPresenting: presenting) { result, error in
                // Google のトークンはサーバーが引き換えて持つので、端末には残さない。
                signIn.signOut()
                if let error = error as NSError? {
                    if error.domain == kGIDSignInErrorDomain && error.code == GIDSignInError.canceled.rawValue {
                        call.rejectCanceled()
                    } else {
                        call.reject(error)
                    }
                    return
                }
                guard let code = result?.serverAuthCode else {
                    call.reject("serverAuthCode を受け取れませんでした")
                    return
                }
                call.resolve(["serverAuthCode": code])
            }
        }
    }

    /// LINE でログインし、ID トークンとアクセストークンを返す。`nonce` はサーバーが発行したもの (`/app/auth/nonce`)。
    /// 閉じられたら `CANCELED` で失敗させる。
    @objc func signInWithLine(_ call: CAPPluginCall) {
        guard let channelId = call.getString("channelId"), let nonce = call.getString("nonce") else {
            call.reject("channelId と nonce が要ります")
            return
        }
        DispatchQueue.main.async {
            let manager = LoginManager.shared
            // SDK は1つのチャネルにしか設定できないので、つなぐ先を変えたときだけ設定し直す。
            if self.lineChannelId != channelId {
                if manager.isSetupFinished { manager.reset() }
                manager.setup(channelID: channelId, universalLinkURL: nil)
                self.lineChannelId = channelId
            }
            var parameters = LoginManager.Parameters()
            parameters.IDTokenNonce = nonce
            manager.login(
                permissions: [.openID, .profile, .email], in: self.bridge?.viewController, parameters: parameters
            ) { result in
                switch result {
                case .success(let login):
                    guard let idToken = login.accessToken.IDTokenRaw else {
                        call.reject("ID トークンを受け取れませんでした")
                        return
                    }
                    call.resolve(["idToken": idToken, "accessToken": login.accessToken.value])
                case .failure(let error) where error.isUserCancelled:
                    call.rejectCanceled()
                case .failure(let error):
                    call.reject(error)
                }
            }
        }
    }

    /// LINE SDK を設定したチャネル。
    private var lineChannelId: String?

    /// Sign in with Apple で認可コードを得る。`nonce` はサーバーが発行したもの (`/app/auth/nonce`)。
    /// 名前は最初の承認のときだけ返る。閉じられたら `CANCELED` で失敗させる。
    /// アプリに Sign in with Apple の entitlement が要る (Developer Program の登録のあとに足す。docs/mobile-app.md)。
    @objc func signInWithApple(_ call: CAPPluginCall) {
        guard let nonce = call.getString("nonce") else {
            call.reject("nonce が要ります")
            return
        }
        // 結果が届いたら手放す。delegate が持つので、プラグインへは弱い参照にして輪を作らない。
        let release: () -> Void = { [weak self] in self?.appleSignIn = nil }
        DispatchQueue.main.async {
            guard let window = self.bridge?.webView?.window else {
                call.reject("画面を出せません")
                return
            }
            let request = ASAuthorizationAppleIDProvider().createRequest()
            request.requestedScopes = [.fullName, .email]
            // サーバーは ID トークンの nonce を発行した値そのものと照らすので、ハッシュにせず渡す。
            request.nonce = nonce
            let controller = ASAuthorizationController(authorizationRequests: [request])
            let delegate = AppleSignInDelegate(call: call, anchor: window, finish: release)
            // 結果が届くまで、コントローラーと delegate を持っておく (どちらも弱い参照でしか持たれないため)。
            self.appleSignIn = (controller, delegate)
            controller.delegate = delegate
            controller.presentationContextProvider = delegate
            controller.performRequests()
        }
    }

    /// 進行中の Sign in with Apple。
    private var appleSignIn: (ASAuthorizationController, AppleSignInDelegate)?
}

/// Sign in with Apple の結果を `CAPPluginCall` に返す。
private class AppleSignInDelegate: NSObject, ASAuthorizationControllerDelegate,
    ASAuthorizationControllerPresentationContextProviding
{
    private let call: CAPPluginCall
    private let anchor: UIWindow
    private let finish: () -> Void

    init(call: CAPPluginCall, anchor: UIWindow, finish: @escaping () -> Void) {
        self.call = call
        self.anchor = anchor
        self.finish = finish
    }

    func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor {
        anchor
    }

    func authorizationController(
        controller: ASAuthorizationController, didCompleteWithAuthorization authorization: ASAuthorization
    ) {
        defer { finish() }
        guard let credential = authorization.credential as? ASAuthorizationAppleIDCredential,
            let codeData = credential.authorizationCode,
            let code = String(data: codeData, encoding: .utf8)
        else {
            call.reject("認可コードを受け取れませんでした")
            return
        }
        var result: [String: Any] = ["authorizationCode": code]
        if let givenName = credential.fullName?.givenName { result["givenName"] = givenName }
        if let familyName = credential.fullName?.familyName { result["familyName"] = familyName }
        call.resolve(result)
    }

    func authorizationController(controller: ASAuthorizationController, didCompleteWithError error: Error) {
        defer { finish() }
        if (error as? ASAuthorizationError)?.code == .canceled {
            call.rejectCanceled()
        } else {
            call.reject(error)
        }
    }
}

private extension CAPPluginCall {
    /// 利用者が画面を閉じたときの失敗。JS 側は `isSignInCanceled` で見分ける。
    func rejectCanceled() {
        reject("キャンセルされました", "CANCELED")
    }

    /// SDK の失敗をそのまま返す。
    func reject(_ error: Error) {
        reject(error.localizedDescription, nil, error)
    }
}
