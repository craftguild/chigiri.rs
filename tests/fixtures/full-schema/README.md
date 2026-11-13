# Full-schema compatibility fixture

Repo-owned synthetic SDL exercises 67 objects, 709 object fields, 19 input
objects, 65 bound enums and 5 custom scalars across 11 files (34,687 bytes).
It retains the large-input, list/nullability/default, root-argument and typed
binding coverage of the retired captured corpus. Names and descriptions describe
fixture data, without requiring another application's types or checkout.

[source.json](source.json) inventories these current inputs and their actual
SHA-256 digests, shape and three independent domain-enum vectors. It contains no
claim that the synthetic input was observed in an external repository. This is
first-party test input under the repository's MIT OR Apache-2.0 licenses.

`generated/` is the ABI 8 crate produced by the real `chigiri rust codegen`
command. Never edit it by hand. Complete generated ownership and isolated exact
reproduction validate all 593 Rust files. `domain/` declares independent
`FormatKind`, `ExternalTransport` and `UnitKind` enums. Their sets contain three,
two and three variants respectively; no extra sentinel variant is added.
`consumer/` connects all 65 enums and every scalar/object associated type through
the public SDK. Rust tests pass external enums through real root arguments and
resolve all three `Measurement` fields through ordinary methods, checking the
round trips and generated ABI.

Run from the repository root after a normal CLI build:

```sh
chigiri rust --manifest tests/fixtures/full-schema/chigiri.toml check
chigiri rust --manifest tests/fixtures/full-schema/chigiri.toml doctor
bash scripts/python-with-toml.sh scripts/full_schema_corpus.py
bash scripts/python-with-toml.sh scripts/test-full-schema-corpus.py
./scripts/validate.sh
```

Private original evaluation material is not a build/test input. Historical
results do not qualify this replacement; run current generation, ownership,
consumer and repository checks. The independent synthetic large-consumer fixture
continues to cover its separate 70-object/750-field contract.
