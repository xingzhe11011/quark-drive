# Security Policy

## Reporting a vulnerability

Please do not open a public issue for credential leaks or exploitable bugs.
Contact the maintainer privately through the security contact configured on
the repository, or open a minimal GitHub Security Advisory draft.

Include the affected version, reproduction steps, and impact. Never include
real cookies, service tickets, QR tokens, or downloaded personal files.

## Scope

This project is an unofficial desktop client. It stores the Quark session
cookie locally in `secrets.json` with mode `0600`; it does not send credentials
to any server other than Quark's own endpoints.

Known limitation: the local secret file is not encrypted at rest. Users on
shared machines should protect their home directory and report suspected
credential exposure by revoking the session from the Quark app.
