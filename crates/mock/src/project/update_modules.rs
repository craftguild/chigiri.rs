//! Compare resolver and hook base/user/next bytes uniformly while preserving user and existing `.dist` ownership.

use std::collections::BTreeMap;

use super::comparison::{Decision, compare};
use super::digest::Digest;
use super::state::Dist;
use super::update_inputs::{Inputs, failure};
use super::{ProjectError, UpdateAction, UpdateReport};
use chigiri_host::PortableRelativePath;

pub(super) struct Revision {
    pub base: Digest,
    pub candidate: Digest,
    pub dist: Option<Dist>,
}

pub(super) fn reconcile(
    path: &PortableRelativePath,
    base: &Digest,
    old_dist: Option<&Dist>,
    files: &mut BTreeMap<PortableRelativePath, String>,
    inputs: &mut Inputs,
    report: &mut UpdateReport,
) -> Result<Revision, ProjectError> {
    let user = inputs.required(path.as_str(), crate::luau::MAX_BODY)?;
    let user_hash = Digest::bytes(&user.bytes);
    let source = files.remove(path).ok_or_else(|| collision(path.as_str()))?;
    let candidate = Digest::bytes(source.as_bytes());
    let decision = compare(base, &user_hash, &candidate);
    let mut next = Revision {
        base: if decision.advances_base() {
            candidate.clone()
        } else {
            base.clone()
        },
        candidate,
        dist: old_dist.cloned(),
    };
    let action = match decision {
        Decision::Unchanged => UpdateAction::Unchanged,
        Decision::Replace => {
            files.insert(path.clone(), source);
            UpdateAction::Replaced
        }
        Decision::Adopt => UpdateAction::Adopted,
        Decision::Keep => UpdateAction::Kept,
        Decision::Propose => {
            next.dist = Some(propose(
                path,
                old_dist,
                &next.candidate,
                source,
                files,
                inputs,
            )?);
            UpdateAction::Proposed
        }
    };
    report.event(action, path);
    if let Some(dist) = &next.dist
        && let Some(size) = inputs.size(dist.path.as_str())?
    {
        let stale = if size > crate::luau::MAX_BODY as u64 {
            true
        } else {
            let current = inputs.required(dist.path.as_str(), crate::luau::MAX_BODY)?;
            !next.candidate.matches(&current.bytes)
        };
        // Report a stale proposal only when the generated candidate no longer matches its recorded digest.
        if stale && !files.contains_key(&dist.path) {
            report.event(UpdateAction::StaleProposal, &dist.path);
        }
    }
    Ok(next)
}

fn propose(
    module: &PortableRelativePath,
    old_dist: Option<&Dist>,
    candidate: &Digest,
    source: String,
    files: &mut BTreeMap<PortableRelativePath, String>,
    inputs: &mut Inputs,
) -> Result<Dist, ProjectError> {
    let path = super::logical(&format!("{module}.dist"))?;
    let current = inputs.read(path.as_str(), crate::luau::MAX_BODY)?;
    if let Some(current) = &current {
        let Some(recorded) = old_dist else {
            return Err(collision(path.as_str()));
        };
        if !recorded.digest.matches(&current.bytes) {
            return Err(collision(path.as_str()));
        }
    }
    if current.is_none_or(|file| file.bytes != source.as_bytes()) {
        files.insert(path.clone(), source);
    }
    Ok(Dist {
        path,
        digest: candidate.clone(),
    })
}

fn collision(path: &str) -> ProjectError {
    failure(path, "ownership", "managed or edited destination collision")
}
