# Security policy

## Supported versions

No Chigiri version has yet completed the first production release. Security fixes are prepared for the current 0.1.x
candidate branch only. After the first Stable release, the latest minor release receives security fixes; older minor
releases are unsupported unless a release note explicitly says otherwise.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability. Send a private report to `security@progressiveworks.co`
with the affected version or commit, reproduction conditions, impact, and a safe way to contact the reporter. Do not
include production credentials, personal data, or other secrets.

Maintainers should acknowledge a report within three business days, classify severity and affected versions, and
provide a remediation status within ten business days. A fix is validated with the relevant conformance, resource,
fuzz regression, and package gates before coordinated disclosure. Credit and disclosure timing are agreed with the
reporter when practical.

## Advisory process

Confirmed vulnerabilities receive a private fix branch, regression test with sanitized input, affected-version
inventory, and advisory text. External publication, registry release, Git tag, and advisory publication require
separate explicit release authority and are not performed by the local candidate workflow.
