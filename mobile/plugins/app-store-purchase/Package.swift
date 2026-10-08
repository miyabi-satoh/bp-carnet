// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "BpCarnetAppStorePurchase",
    platforms: [.iOS(.v16)],
    products: [
        .library(
            name: "BpCarnetAppStorePurchase",
            targets: ["AppStorePurchasePlugin"])
    ],
    dependencies: [
        .package(url: "https://github.com/ionic-team/capacitor-swift-pm.git", from: "8.0.0")
    ],
    targets: [
        .target(
            name: "AppStorePurchasePlugin",
            dependencies: [
                .product(name: "Capacitor", package: "capacitor-swift-pm")
            ],
            path: "ios/Sources/AppStorePurchasePlugin")
    ]
)
