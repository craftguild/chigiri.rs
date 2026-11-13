# Recovering a generated crate

Status: Implemented. This procedure describes recovery for the sibling
generator transaction and the migrated ordinary CLI.

When `codegen` or `update` fails, first inspect the diagnostic for the message
that the new generated crate was installed as the live tree. That message means
post-commit cleanup failed and the new crate is already at the output path. A
pre-commit failure attempts to roll back to the old tree and reports any
rollback failure alongside the primary cause. Fully automatic recovery from
SIGKILL or power loss is not guaranteed.

Generation stops if any of these artifacts remain in the output's parent
directory. Other outputs under the same parent are also blocked until recovery
is complete.

| Artifact | Contents |
| --- | --- |
| `.chigiri-transaction/state.json` | The target output name and whether the operation was `codegen` or `update`; it does not infer the commit result. |
| `.chigiri-transaction/candidate` | The new candidate, which may have failed a pre-commit check. |
| `.chigiri-transaction/compile` | Sources and lockfile for the private Cargo check. |
| `.chigiri-transaction/previous` | The renamed old tree; it may be partly deleted if cleanup failed. |
| `.chigiri-recovery` | A recovery archive containing every old file byte and the target output name; it is fully written and synchronized before backup deletion begins. |

Stop any running generator before inspecting the live tree and residual
artifacts. Do not delete all residual artifacts before recovery. If writing the
archive itself failed, the archive may be partial, but deletion of `previous`
has not yet begun. A failure after backup deletion starts preserves a complete
archive.

When an archive exists, pass it and an absent extraction destination to the
Lab01 recovery script. Create the destination's parent first.

```sh
python3 scripts/recover-generated-crate.py \
  path/to/generated/.chigiri-recovery \
  path/to/generated/recovered-contract
```

The script enforces archive bounds and structure, verifies every SHA-256 in the
marker inventory, and handles paths without following links. It rejects an
existing destination of any kind: file, directory, or link. It deliberately
leaves a partial extraction in place on failure so the archive remains
available for inspection. Recovery restores old bytes; it does not establish
compatibility with the current SDL or manifest.

After checking the recovered copy, preserve the new live tree under another
name and move the recovered copy to the original output path if the old tree
must be restored. If no archive exists but a complete pre-rollback `previous`
tree remains, copy it before taking other action. If the new live tree will be
kept, review its generated contents and the required consumer changes. Remove
the transaction artifacts and retry only after deciding which tree to retain
and where to preserve it. `.chigiri-cache` is a reusable Cargo cache, not a
recovery archive.
