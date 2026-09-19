# Security Policy

## Reporting a vulnerability

Please do **not** open a public issue for security vulnerabilities.

Report them privately via
[GitHub Security Advisories](https://github.com/tero-k/verkkokyyla/security/advisories/new)
or by email to tero@kiminki.fi. You can expect an acknowledgement within a
few days.

## Supported versions

Only the latest release receives security fixes.

## Security model notes

Things worth knowing when evaluating or using Verkkokyylä:

- **Credentials**: MikroTik passwords are stored in the operating system
  keyring (Windows Credential Manager, macOS Keychain, or a Linux Secret
  Service provider such as gnome-keyring/KWallet), never in the app database
  or on disk in plaintext. On Linux, credential storage requires a running
  DBus Secret Service.
- **Local data**: session history lives in a local SQLite database under the
  OS app-data directory. It contains measurement data and connection
  metadata (hosts, usernames), but no passwords.
- **SSH host keys**: for typical LAN-managed routers, the app accepts unknown
  SSH host keys on first connection instead of requiring a pre-seeded
  known-hosts entry. This is a deliberate usability trade-off documented in
  the README; be aware of it on untrusted networks.
- **Router access**: MikroTik features use the RouterOS REST API (HTTPS via
  `www-ssl`, or HTTP via `www` on RouterOS v7.9+) and SSH for terminals and
  backups. Use a dedicated, least-privilege router account where possible.
