// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "BpCarnetNativeLogin",
    platforms: [.iOS(.v15)],
    products: [
        .library(
            name: "BpCarnetNativeLogin",
            targets: ["NativeLoginPlugin"])
    ],
    dependencies: [
        .package(url: "https://github.com/ionic-team/capacitor-swift-pm.git", from: "8.0.0"),
        .package(url: "https://github.com/google/GoogleSignIn-iOS.git", .upToNextMajor(from: "10.0.0")),
        .package(url: "https://github.com/line/line-sdk-ios-swift.git", .upToNextMajor(from: "5.17.0"))
    ],
    targets: [
        .target(
            name: "NativeLoginPlugin",
            dependencies: [
                .product(name: "Capacitor", package: "capacitor-swift-pm"),
                .product(name: "GoogleSignIn", package: "GoogleSignIn-iOS"),
                .product(name: "LineSDK", package: "line-sdk-ios-swift")
            ],
            path: "ios/Sources/NativeLoginPlugin")
    ]
)
