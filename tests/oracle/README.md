# Vendored bash oracle

A **frozen snapshot** of the original bash token-aware scheduler from
`idea-to-production/plugins/concierge/scheduler/`, captured at the commit recorded in
[`SOURCE_SHA`](./SOURCE_SHA) (`0b46ff3`).

## Why it's here

`tests/conformance.sh` proves `tf` reproduces the bash scheduler byte-for-byte by running the
**same inputs through both**. That requires the bash to exist. The scheduler was **removed** from
idea-to-production (it lives here now, as `tf`), so the oracle can no longer be diffed against a
live checkout — it is vendored here instead, immutable, at the SHA the port was validated against.

Do not edit these scripts. They are the historical reference the port is measured against; changing
them would invalidate the conformance proof. To re-validate against a newer bash, override
`BASH_DIR` and update `SOURCE_SHA`.
