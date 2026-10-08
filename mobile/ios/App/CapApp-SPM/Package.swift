// swift-tools-version: 5.9
import PackageDescription

// DO NOT MODIFY THIS FILE - managed by Capacitor CLI commands
let package = Package(
    name: "CapApp-SPM",
    platforms: [.iOS(.v16)],
    products: [
        .library(
            name: "CapApp-SPM",
            targets: ["CapApp-SPM"])
    ],
    dependencies: [
        .package(url: "https://github.com/ionic-team/capacitor-swift-pm.git", exact: "8.5.2"),
        .package(name: "AparajitaCapacitorSecureStorage", path: "../../../../frontend/node_modules/.pnpm/@aparajita+capacitor-secure-storage@8.0.1/node_modules/@aparajita/capacitor-secure-storage"),
        .package(name: "CapacitorApp", path: "../../../../frontend/node_modules/.pnpm/@capacitor+app@8.1.1_@capacitor+core@8.5.2/node_modules/@capacitor/app"),
        .package(name: "CapacitorFilesystem", path: "../../../../frontend/node_modules/.pnpm/@capacitor+filesystem@8.1.3_@capacitor+core@8.5.2/node_modules/@capacitor/filesystem"),
        .package(name: "CapacitorShare", path: "../../../../frontend/node_modules/.pnpm/@capacitor+share@8.0.2_@capacitor+core@8.5.2/node_modules/@capacitor/share"),
        .package(name: "CapgoCapacitorPrinter", path: "../../../../frontend/node_modules/.pnpm/@capgo+capacitor-printer@8.1.4_@capacitor+core@8.5.2/node_modules/@capgo/capacitor-printer"),
        .package(name: "BpCarnetAppStorePurchase", path: "../../../plugins/app-store-purchase"),
        .package(name: "BpCarnetNativeLogin", path: "../../../plugins/native-login")
    ],
    targets: [
        .target(
            name: "CapApp-SPM",
            dependencies: [
                .product(name: "Capacitor", package: "capacitor-swift-pm"),
                .product(name: "Cordova", package: "capacitor-swift-pm"),
                .product(name: "AparajitaCapacitorSecureStorage", package: "AparajitaCapacitorSecureStorage"),
                .product(name: "CapacitorApp", package: "CapacitorApp"),
                .product(name: "CapacitorFilesystem", package: "CapacitorFilesystem"),
                .product(name: "CapacitorShare", package: "CapacitorShare"),
                .product(name: "CapgoCapacitorPrinter", package: "CapgoCapacitorPrinter"),
                .product(name: "BpCarnetAppStorePurchase", package: "BpCarnetAppStorePurchase"),
                .product(name: "BpCarnetNativeLogin", package: "BpCarnetNativeLogin")
            ]
        )
    ]
)
