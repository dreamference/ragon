"""The node's own certificate authority and the gate's TLS (specs/DREAMFERENCE_AUTH.md §6).

Each node gets a self-signed CA, created once and never rewritten (rotation is a later item), and
a server certificate for its names signed by it, under `~/.local/share/dreamference/tls/`, keys
mode 0600. The gate on the model port and `ragon web` serve that certificate.

The host creates the files (`ensure`, with the `cryptography` package, imported only there); the
gate's container imports this file too, for the TLS context and the first-byte sniff, so at
import it uses the standard library only.
"""

import asyncio
import contextlib
import datetime
import hashlib
import ipaddress
import os
import shutil
import socket
import ssl
from pathlib import Path
from typing import Any, Final

CA_CERT: Final[str] = "ca.pem"
CA_KEY: Final[str] = "ca.key"
SERVER_CERT: Final[str] = "server.pem"
SERVER_KEY: Final[str] = "server.key"
# The files the gate's container gets: never the CA's key.
STAGED_FILES: Final[tuple[str, ...]] = (SERVER_CERT, SERVER_KEY, CA_CERT)
# The directory beside the gate's state that holds them.
STAGED_DIR: Final[str] = "tls"
SECRET_FILES: Final[tuple[str, ...]] = (CA_KEY, SERVER_KEY)
PRIVATE_MODE: Final[int] = 0o600
PRIVATE_DIR_MODE: Final[int] = 0o700
CA_DAYS: Final[int] = 3650
SERVER_DAYS: Final[int] = 825
# A server certificate this close to its end is issued again (by the same CA).
RENEW_DAYS: Final[int] = 30
LOOPBACK_NAME: Final[str] = "localhost"
LOOPBACK_ADDRESSES: Final[tuple[str, ...]] = ("127.0.0.1", "::1")
CA_NAME: Final[str] = "Ragon node CA"
# The first byte of a TLS record carrying a handshake (a ClientHello); plain HTTP starts with a
# method's letter.
TLS_HANDSHAKE: Final[int] = 0x16
SNIFF_TIMEOUT_S: Final[float] = 30.0
HANDSHAKE_TIMEOUT_S: Final[float] = 30.0
ALPN: Final[tuple[str, ...]] = ("http/1.1",)


class ModelGateTls:
    """Creates the node's CA and server certificate, and serves TLS beside plain HTTP."""

    @classmethod
    def directory(cls) -> Path:
        """Where the node's CA and server certificate live.

        Returns:
            Path: `~/.local/share/dreamference/tls`, resolved at call time.
        """
        return Path("~/.local/share/dreamference/tls").expanduser()

    @classmethod
    def names(cls) -> list[str]:
        """The names the server certificate covers: `<node>.local`, the host name, `localhost`.

        Returns:
            list[str]: DNS names, without repeats.
        """
        host = socket.gethostname().strip().lower().rstrip(".")
        short = host.split(".", 1)[0]
        wanted = [f"{short}.local", host, LOOPBACK_NAME] if short else [LOOPBACK_NAME]
        return list(dict.fromkeys(wanted))

    @classmethod
    def ensure(cls, directory: Path | None = None, names: list[str] | None = None) -> Path:
        """Creates the CA once and a server certificate for `names` when it has none that fits.

        The CA is never replaced: a client that pinned it must find it again after an update.

        Args:
            directory: Where the files go; `directory()` unless named.
            names: The server certificate's DNS names; `names()` unless named.

        Returns:
            Path: The directory.

        Raises:
            ImportError: Without the `cryptography` package.
            OSError: When a file cannot be written.
        """
        folder = directory or cls.directory()
        wanted = names or cls.names()
        folder.mkdir(parents=True, exist_ok=True)
        Path(folder).chmod(PRIVATE_DIR_MODE)
        if not (folder / CA_CERT).exists() or not (folder / CA_KEY).exists():
            cls.create_ca(folder)
        if cls.needs_server_certificate(folder, wanted):
            cls.create_server_certificate(folder, wanted)
        return folder

    @classmethod
    def write_private(cls, path: Path, data: bytes) -> None:
        """Writes a key file created with mode 0600, never readable by others for a moment.

        Args:
            path: The file; an existing one is replaced.
            data: Its content.
        """
        staging = path.with_name(f".{path.name}.{os.getpid()}.tmp")
        with contextlib.suppress(FileNotFoundError):
            staging.unlink()
        fd = os.open(staging, os.O_WRONLY | os.O_CREAT | os.O_EXCL, PRIVATE_MODE)
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
        staging.replace(path)

    @classmethod
    def create_ca(cls, folder: Path) -> None:
        """Creates the CA's key and its self-signed certificate (ten years, EC P-256).

        Args:
            folder: The TLS directory.
        """
        from cryptography import x509
        from cryptography.hazmat.primitives import hashes, serialization
        from cryptography.hazmat.primitives.asymmetric import ec
        from cryptography.x509.oid import NameOID

        key = ec.generate_private_key(ec.SECP256R1())
        host = socket.gethostname().split(".", 1)[0]
        name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, f"{CA_NAME} {host}"[:64])])
        now = datetime.datetime.now(datetime.UTC)
        public = key.public_key()
        certificate = (
            x509.CertificateBuilder()
            .subject_name(name)
            .issuer_name(name)
            .public_key(public)
            .serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(minutes=5))
            .not_valid_after(now + datetime.timedelta(days=CA_DAYS))
            .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
            .add_extension(
                x509.KeyUsage(
                    digital_signature=False,
                    content_commitment=False,
                    key_encipherment=False,
                    data_encipherment=False,
                    key_agreement=False,
                    key_cert_sign=True,
                    crl_sign=True,
                    encipher_only=False,
                    decipher_only=False,
                ),
                critical=True,
            )
            .add_extension(x509.SubjectKeyIdentifier.from_public_key(public), critical=False)
            .sign(key, hashes.SHA256())
        )
        cls.write_private(
            folder / CA_KEY,
            key.private_bytes(
                serialization.Encoding.PEM,
                serialization.PrivateFormat.PKCS8,
                serialization.NoEncryption(),
            ),
        )
        (folder / CA_CERT).write_bytes(certificate.public_bytes(serialization.Encoding.PEM))
        # A new CA makes any earlier server certificate one nobody can verify.
        with contextlib.suppress(FileNotFoundError):
            (folder / SERVER_CERT).unlink()

    @classmethod
    def needs_server_certificate(cls, folder: Path, names: list[str]) -> bool:
        """True when the server certificate is missing, misses a name or is about to end.

        Args:
            folder: The TLS directory.
            names: The DNS names it must cover.

        Returns:
            bool: Whether to issue it again.
        """
        if not (folder / SERVER_CERT).exists() or not (folder / SERVER_KEY).exists():
            return True
        from cryptography import x509

        try:
            certificate = x509.load_pem_x509_certificate((folder / SERVER_CERT).read_bytes())
            ca = x509.load_pem_x509_certificate((folder / CA_CERT).read_bytes())
            covered = certificate.extensions.get_extension_for_class(
                x509.SubjectAlternativeName
            ).value.get_values_for_type(x509.DNSName)
        except (ValueError, x509.ExtensionNotFound):
            return True
        left = certificate.not_valid_after_utc - datetime.datetime.now(datetime.UTC)
        return (
            certificate.issuer != ca.subject
            or not set(names) <= set(covered)
            or left < datetime.timedelta(days=RENEW_DAYS)
        )

    @classmethod
    def create_server_certificate(cls, folder: Path, names: list[str]) -> None:
        """Issues the server certificate for `names`, `127.0.0.1` and `::1` (825 days).

        Args:
            folder: The TLS directory, holding the CA.
            names: Its DNS names.
        """
        from cryptography import x509
        from cryptography.hazmat.primitives import hashes, serialization
        from cryptography.hazmat.primitives.asymmetric import ec
        from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

        ca_key: Any = serialization.load_pem_private_key((folder / CA_KEY).read_bytes(), None)
        ca = x509.load_pem_x509_certificate((folder / CA_CERT).read_bytes())
        key = ec.generate_private_key(ec.SECP256R1())
        now = datetime.datetime.now(datetime.UTC)
        alternatives: list[x509.GeneralName] = [x509.DNSName(name) for name in names]
        alternatives += [
            x509.IPAddress(ipaddress.ip_address(address)) for address in LOOPBACK_ADDRESSES
        ]
        certificate = (
            x509.CertificateBuilder()
            .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, names[0][:64])]))
            .issuer_name(ca.subject)
            .public_key(key.public_key())
            .serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(minutes=5))
            .not_valid_after(now + datetime.timedelta(days=SERVER_DAYS))
            .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
            .add_extension(x509.SubjectAlternativeName(alternatives), critical=False)
            .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
            .add_extension(
                x509.AuthorityKeyIdentifier.from_issuer_public_key(ca_key.public_key()),
                critical=False,
            )
            .sign(ca_key, hashes.SHA256())
        )
        cls.write_private(
            folder / SERVER_KEY,
            key.private_bytes(
                serialization.Encoding.PEM,
                serialization.PrivateFormat.PKCS8,
                serialization.NoEncryption(),
            ),
        )
        (folder / SERVER_CERT).write_bytes(certificate.public_bytes(serialization.Encoding.PEM))

    @classmethod
    def stage(cls, folder: Path, gate_dir: Path) -> bool:
        """Copies the server certificate, its key and the CA certificate beside the gate's state.

        The CA's key never leaves `folder`. Without a complete set, the staged copy is removed,
        so the gate serves plain HTTP only.

        Args:
            folder: The TLS directory.
            gate_dir: The gate's state directory, which its container mounts.

        Returns:
            bool: Whether the gate has what it needs to serve TLS.
        """
        target = gate_dir / STAGED_DIR
        if not all((folder / name).is_file() for name in STAGED_FILES):
            shutil.rmtree(target, ignore_errors=True)
            return False
        target.mkdir(parents=True, exist_ok=True)
        Path(target).chmod(PRIVATE_DIR_MODE)
        for name in STAGED_FILES:
            data = (folder / name).read_bytes()
            if name in SECRET_FILES:
                cls.write_private(target / name, data)
            else:
                (target / name).write_bytes(data)
        return True

    @classmethod
    def fingerprint(cls, pem: bytes) -> str:
        """The SHA-256 fingerprint of a PEM certificate, as `openssl x509 -fingerprint` prints it.

        Args:
            pem: The certificate.

        Returns:
            str: Upper-case hex pairs joined by colons.
        """
        der = ssl.PEM_cert_to_DER_cert(pem.decode("ascii"))
        digest = hashlib.sha256(der).hexdigest().upper()
        return ":".join(digest[i : i + 2] for i in range(0, len(digest), 2))

    @classmethod
    def server_context(cls, folder: str | Path) -> ssl.SSLContext | None:
        """The gate's TLS context, from `server.pem` and `server.key` in `folder`.

        Args:
            folder: A directory holding them (the staged copy inside the container).

        Returns:
            ssl.SSLContext | None: The context, or None when the files are missing or unusable.
        """
        certificate, key = Path(folder) / SERVER_CERT, Path(folder) / SERVER_KEY
        if not certificate.is_file() or not key.is_file():
            return None
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.minimum_version = ssl.TLSVersion.TLSv1_2
        context.set_alpn_protocols(list(ALPN))
        try:
            context.load_cert_chain(str(certificate), str(key))
        except (OSError, ssl.SSLError):
            return None
        return context

    @classmethod
    def wake(cls, ready: "asyncio.Future[None]") -> None:
        """Resolves `ready` once, when the socket became readable."""
        if not ready.done():
            ready.set_result(None)

    @classmethod
    def is_tls(cls, first: bytes) -> bool:
        """True when a connection's first byte starts a TLS handshake rather than plain HTTP."""
        return bool(first) and first[0] == TLS_HANDSHAKE

    @classmethod
    async def peek(cls, connection: socket.socket) -> bytes:
        """The connection's first byte, left in place for whoever reads it next.

        Args:
            connection: An accepted, non-blocking socket.

        Returns:
            bytes: One byte, or nothing when the client closed first.
        """
        loop = asyncio.get_running_loop()
        while True:
            try:
                return connection.recv(1, socket.MSG_PEEK)
            except (BlockingIOError, InterruptedError):
                pass
            ready: asyncio.Future[None] = loop.create_future()
            loop.add_reader(connection.fileno(), cls.wake, ready)
            try:
                await ready
            finally:
                loop.remove_reader(connection.fileno())
