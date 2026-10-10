import CryptoKit
import Foundation

private let sparkleNamespace = "http://www.andymatuschak.org/xml-namespaces/sparkle"

private enum TestFailure: Error {
    case assertion
}

private func require(_ condition: Bool) throws {
    if !condition {
        throw TestFailure.assertion
    }
}

private func writeInfoPlist(encodedPublicKey: String?, to url: URL) throws {
    var plist: [String: String] = [:]
    if let encodedPublicKey {
        plist["SUPublicEDKey"] = encodedPublicKey
    }
    let data = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
    try data.write(to: url)
}

private func writeAppcast(
    encodedSignature: String?,
    archiveSize: Int,
    prefix: String,
    reboundNamespace: String? = nil,
    to url: URL
) throws {
    let signatureAttribute = encodedSignature.map { " \(prefix):edSignature=\"\($0)\"" } ?? ""
    let itemNamespace = reboundNamespace.map { " xmlns:\(prefix)=\"\($0)\"" } ?? ""
    let xml = """
    <?xml version="1.0" encoding="UTF-8"?>
    <rss xmlns:\(prefix)="\(sparkleNamespace)">
      <channel><item\(itemNamespace)><enclosure url="https://updates.invalid/Tidegauge-1.2.3.zip"
        length="\(archiveSize)"\(signatureAttribute) /></item></channel>
    </rss>
    """
    try Data(xml.utf8).write(to: url)
}

private func runVerifier(
    swift: URL,
    verifier: URL,
    appcast: URL,
    archive: URL,
    infoPlist: URL
) throws -> Int32 {
    let process = Process()
    process.executableURL = swift
    process.arguments = [verifier.path, appcast.path, archive.path, infoPlist.path]
    process.standardOutput = FileHandle.nullDevice
    process.standardError = FileHandle.nullDevice
    try process.run()
    process.waitUntilExit()
    return process.terminationStatus
}

do {
    guard CommandLine.arguments.count == 3 else {
        throw TestFailure.assertion
    }
    let verifierURL = URL(fileURLWithPath: CommandLine.arguments[1])
    let swiftURL = URL(fileURLWithPath: CommandLine.arguments[2])
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }

    let archiveURL = directory.appendingPathComponent("Tidegauge-1.2.3.zip")
    let appcastURL = directory.appendingPathComponent("appcast.xml")
    let infoPlistURL = directory.appendingPathComponent("Info.plist")
    let archiveData = Data("generated archive bytes".utf8)
    try archiveData.write(to: archiveURL)

    let signingKey = Curve25519.Signing.PrivateKey()
    let signature = try signingKey.signature(for: archiveData)
    let encodedPublicKey = signingKey.publicKey.rawRepresentation.base64EncodedString()
    let encodedSignature = signature.base64EncodedString()
    try writeInfoPlist(encodedPublicKey: encodedPublicKey, to: infoPlistURL)
    try writeAppcast(
        encodedSignature: encodedSignature,
        archiveSize: archiveData.count,
        prefix: "alternate",
        to: appcastURL
    )
    try require(
        try runVerifier(
            swift: swiftURL,
            verifier: verifierURL,
            appcast: appcastURL,
            archive: archiveURL,
            infoPlist: infoPlistURL
        ) == 0
    )

    try writeAppcast(
        encodedSignature: encodedSignature,
        archiveSize: archiveData.count,
        prefix: "alternate",
        reboundNamespace: "urn:other",
        to: appcastURL
    )
    try require(
        try runVerifier(
            swift: swiftURL,
            verifier: verifierURL,
            appcast: appcastURL,
            archive: archiveURL,
            infoPlist: infoPlistURL
        ) != 0
    )

    try writeAppcast(
        encodedSignature: nil,
        archiveSize: archiveData.count,
        prefix: "alternate",
        to: appcastURL
    )
    try require(
        try runVerifier(
            swift: swiftURL,
            verifier: verifierURL,
            appcast: appcastURL,
            archive: archiveURL,
            infoPlist: infoPlistURL
        ) != 0
    )
    try writeAppcast(
        encodedSignature: "not-base64!",
        archiveSize: archiveData.count,
        prefix: "alternate",
        to: appcastURL
    )
    try require(
        try runVerifier(
            swift: swiftURL,
            verifier: verifierURL,
            appcast: appcastURL,
            archive: archiveURL,
            infoPlist: infoPlistURL
        ) != 0
    )
    try writeAppcast(
        encodedSignature: encodedSignature,
        archiveSize: archiveData.count,
        prefix: "alternate",
        to: appcastURL
    )
    try writeInfoPlist(encodedPublicKey: "not-base64!", to: infoPlistURL)
    try require(
        try runVerifier(
            swift: swiftURL,
            verifier: verifierURL,
            appcast: appcastURL,
            archive: archiveURL,
            infoPlist: infoPlistURL
        ) != 0
    )
    try writeInfoPlist(encodedPublicKey: nil, to: infoPlistURL)
    try require(
        try runVerifier(
            swift: swiftURL,
            verifier: verifierURL,
            appcast: appcastURL,
            archive: archiveURL,
            infoPlist: infoPlistURL
        ) != 0
    )

    try writeInfoPlist(encodedPublicKey: encodedPublicKey, to: infoPlistURL)
    try Data("tampered archive bytes".utf8).write(to: archiveURL)
    try require(
        try runVerifier(
            swift: swiftURL,
            verifier: verifierURL,
            appcast: appcastURL,
            archive: archiveURL,
            infoPlist: infoPlistURL
        ) != 0
    )

    try archiveData.write(to: archiveURL)
    try writeAppcast(
        encodedSignature: encodedSignature,
        archiveSize: archiveData.count,
        prefix: "alternate",
        to: appcastURL
    )
    let unrelatedKey = Curve25519.Signing.PrivateKey()
    try writeInfoPlist(
        encodedPublicKey: unrelatedKey.publicKey.rawRepresentation.base64EncodedString(),
        to: infoPlistURL
    )
    try require(
        try runVerifier(
            swift: swiftURL,
            verifier: verifierURL,
            appcast: appcastURL,
            archive: archiveURL,
            infoPlist: infoPlistURL
        ) != 0
    )
    print("Generated-key CryptoKit verifier tests passed.")
} catch {
    fputs("Generated-key CryptoKit verifier test failed\n", stderr)
    exit(EXIT_FAILURE)
}
