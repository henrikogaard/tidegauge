import CryptoKit
import Foundation

private let sparkleNamespace = "http://www.andymatuschak.org/xml-namespaces/sparkle"

private struct Enclosure {
    let url: String?
    let signature: String?
}

private final class AppcastParser: NSObject, XMLParserDelegate {
    private var namespaceMappings: [String: [String]] = [:]
    private(set) var enclosures: [Enclosure] = []

    func parser(_ parser: XMLParser, didStartMappingPrefix prefix: String, toURI namespaceURI: String) {
        namespaceMappings[prefix, default: []].append(namespaceURI)
    }

    func parser(_ parser: XMLParser, didEndMappingPrefix prefix: String) {
        guard var mappings = namespaceMappings[prefix], !mappings.isEmpty else {
            return
        }
        mappings.removeLast()
        if mappings.isEmpty {
            namespaceMappings.removeValue(forKey: prefix)
        } else {
            namespaceMappings[prefix] = mappings
        }
    }

    func parser(
        _ parser: XMLParser,
        didStartElement elementName: String,
        namespaceURI: String?,
        qualifiedName qName: String?,
        attributes attributeDict: [String: String] = [:]
    ) {
        guard elementName == "enclosure", namespaceURI == nil || namespaceURI == "" else {
            return
        }
        let signature = namespaceMappings
            .filter { $0.value.last == sparkleNamespace }
            .compactMap { attributeDict["\($0.key):edSignature"] }
            .first
        enclosures.append(Enclosure(url: attributeDict["url"], signature: signature))
    }
}

private func publicKey(from infoPlistURL: URL) throws -> Curve25519.Signing.PublicKey {
    let data = try Data(contentsOf: infoPlistURL)
    let propertyList = try PropertyListSerialization.propertyList(from: data, options: [], format: nil)
    guard
        let dictionary = propertyList as? [String: Any],
        let encodedKey = dictionary["SUPublicEDKey"] as? String,
        let keyData = Data(base64Encoded: encodedKey),
        keyData.count == 32
    else {
        throw VerificationFailure.invalidInput
    }
    return try Curve25519.Signing.PublicKey(rawRepresentation: keyData)
}

private func isCurrentArchiveURL(_ string: String?, archiveURL: URL) -> Bool {
    guard
        let string,
        let components = URLComponents(string: string),
        let path = components.percentEncodedPath.removingPercentEncoding
    else {
        return false
    }
    return URL(fileURLWithPath: path).lastPathComponent == archiveURL.lastPathComponent
}

private enum VerificationFailure: Error {
    case invalidInput
}

private func verify(appcastURL: URL, archiveURL: URL, infoPlistURL: URL) throws {
    let xml = try Data(contentsOf: appcastURL)
    let parser = XMLParser(data: xml)
    parser.shouldProcessNamespaces = true
    parser.shouldReportNamespacePrefixes = true
    let delegate = AppcastParser()
    parser.delegate = delegate
    guard parser.parse(), !delegate.enclosures.isEmpty else {
        throw VerificationFailure.invalidInput
    }

    let key = try publicKey(from: infoPlistURL)
    let archive = try Data(contentsOf: archiveURL, options: .mappedIfSafe)
    var foundCurrentArchive = false
    for enclosure in delegate.enclosures where isCurrentArchiveURL(enclosure.url, archiveURL: archiveURL) {
        foundCurrentArchive = true
        guard
            let encodedSignature = enclosure.signature,
            let signature = Data(base64Encoded: encodedSignature),
            signature.count == 64,
            key.isValidSignature(signature, for: archive)
        else {
            throw VerificationFailure.invalidInput
        }
    }
    guard foundCurrentArchive else {
        throw VerificationFailure.invalidInput
    }
}

private func fail() -> Never {
    fputs("Appcast signature verification failed\n", stderr)
    exit(EXIT_FAILURE)
}

guard CommandLine.arguments.count == 4 else {
    fputs("Usage: verify_appcast_signatures.swift CANDIDATE_XML CURRENT_ZIP APP_INFO_PLIST\n", stderr)
    exit(2)
}

do {
    try verify(
        appcastURL: URL(fileURLWithPath: CommandLine.arguments[1]),
        archiveURL: URL(fileURLWithPath: CommandLine.arguments[2]),
        infoPlistURL: URL(fileURLWithPath: CommandLine.arguments[3])
    )
} catch {
    fail()
}
