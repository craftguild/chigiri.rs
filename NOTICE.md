# Notices

Chigiri copyright © 2026 Chigiri contributors. Project-owned source is available
under MIT OR Apache-2.0; choose either license in [LICENSE](LICENSE).

Third-party source, fonts and dependencies retain their original terms.
[Vendored source provenance](vendor/README.md) records the scoped formatter
license exceptions and unshipped standards-tool dependency patch.
[CLI artwork attribution](crates/cli/assets/NOTICE.md) retains the FIGlet license.
Embedded GraphiQL licenses and notices are included in
[the asset payload](crates/graphiql/assets/THIRD_PARTY_NOTICES.txt).

`chigiri sbom` prints the exact embedded CycloneDX 1.5 document for that binary's
selected target, profile, features and runtime dependencies, including actual
bundled npm contributions and supplemental assets. The Cargo dependency policy
also audits build/development dependencies in the resolved workspace. These
checks identify their respective scopes; they do not establish unexecuted
platform runtime qualification or authorize publication.
