import base64
import binascii
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from urllib.parse import unquote, urlsplit


SPARKLE_NAMESPACE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
SIGNATURE_ATTRIBUTE = f"{{{SPARKLE_NAMESPACE}}}edSignature"


class AppcastValidationError(ValueError):
    pass


def validate_appcast(candidate_xml, current_zip):
    try:
        root = ET.parse(candidate_xml).getroot()
    except (OSError, ET.ParseError):
        raise AppcastValidationError("invalid appcast XML") from None

    enclosures = [
        element
        for element in root.iter()
        if isinstance(element.tag, str)
        and element.tag.rsplit("}", 1)[-1] == "enclosure"
    ]
    if not enclosures:
        raise AppcastValidationError("appcast has no enclosures")

    try:
        current_size = Path(current_zip).stat().st_size
    except OSError:
        raise AppcastValidationError("current archive is unavailable") from None

    current_name = Path(current_zip).name
    current_archive_found = False
    for enclosure in enclosures:
        signature = enclosure.get(SIGNATURE_ATTRIBUTE)
        if not signature:
            raise AppcastValidationError("an enclosure has no valid EdDSA signature")

        try:
            decoded_signature = base64.b64decode(signature, validate=True)
        except (binascii.Error, ValueError):
            raise AppcastValidationError("an enclosure has no valid EdDSA signature") from None
        if len(decoded_signature) != 64:
            raise AppcastValidationError("an enclosure has no valid EdDSA signature")

        url = enclosure.get("url", "")
        try:
            url_name = unquote(urlsplit(url).path.rsplit("/", 1)[-1])
        except ValueError:
            continue
        if url_name != current_name:
            continue

        length = enclosure.get("length")
        try:
            if int(length, 10) == current_size:
                current_archive_found = True
        except (TypeError, ValueError):
            pass

    if not current_archive_found:
        raise AppcastValidationError("current archive is not represented in the appcast")


def main(argv=None):
    arguments = sys.argv[1:] if argv is None else argv
    if len(arguments) != 2:
        print("Usage: validate_appcast.py CANDIDATE_XML CURRENT_ZIP", file=sys.stderr)
        return 2

    try:
        validate_appcast(arguments[0], arguments[1])
    except AppcastValidationError as error:
        print(f"Appcast validation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
