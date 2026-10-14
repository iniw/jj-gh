//! PR lookup helpers: resolve a revision or PR number into the
//! GitHub PR (and its remote target). Used by the `pr` handlers (`auto-merge`,
//! `edit`, `rerun`) and by `jj-gh debug pr-lookup`.

use crate::{
    gh::{Gh, PrDetails, PrSummary, remote},
    jj::{CommitInfo, Jj},
};
use anyhow::{Result, anyhow, bail};
use std::collections::HashSet;

/// Resolved lookup state for a revision: the bookmark, the remote target, the
/// `owner:branch` head spec, the detected trunk bookmark, and the open PR (if
/// any) whose head matches.
///
/// Shared by `jj-gh pr auto-merge <rev>` and `jj-gh debug pr-lookup`.
#[derive(Debug)]
pub struct PrLookup {
    pub branch: String,
    pub target: remote::Target,
    pub head_spec: String,
    pub default_base: String,
    pub summary: Option<PrSummary>,
}

/// Lookup a PR by either a revision ID or PR number
///
/// # Errors
///
/// Returns an error if `rev` has no local bookmark, if the configured
/// `default_remote` is unset, if `trunk()` is empty, or if any underlying
/// jj/GH call fails.
pub async fn get_pr<J: Jj, G: Gh>(
    jj: &J,
    gh: &G,
    default_remote: &str,
    upstream_remote: &str,
    number_or_rev: &str,
) -> Result<PrDetails> {
    Ok(
        resolve_pr_with_target(jj, gh, default_remote, upstream_remote, number_or_rev)
            .await?
            .0,
    )
}

/// Same as [`get_pr`] but also returns the resolved [`remote::Target`] so
/// callers that need the owner/repo for further API calls don't have to
/// re-derive it from the remote URL.
///
/// # Errors
///
/// See [`get_pr`].
pub async fn resolve_pr_with_target<J: Jj, G: Gh>(
    jj: &J,
    gh: &G,
    default_remote: &str,
    upstream_remote: &str,
    number_or_rev: &str,
) -> Result<(PrDetails, remote::Target)> {
    if let Ok(num) = number_or_rev.parse::<u64>() {
        let origin_url = jj
            .remote_url(default_remote)?
            .ok_or_else(|| anyhow!("`{default_remote}` remote is not configured"))?;
        let upstream_url = jj.remote_url(upstream_remote)?;
        let target = remote::target(&origin_url, upstream_url.as_deref())?;
        let pr = gh.get_pr(&target.owner, &target.repo, num).await?;
        Ok((pr, target))
    } else {
        let lookup =
            resolve_pr_for_rev(jj, gh, default_remote, upstream_remote, number_or_rev).await?;
        let summary = lookup.summary.ok_or_else(|| {
            anyhow!(
                "no open PR for revision `{number_or_rev}` (head `{}`)",
                lookup.head_spec,
            )
        })?;
        let pr = gh
            .get_pr(&lookup.target.owner, &lookup.target.repo, summary.number)
            .await?;
        Ok((pr, lookup.target))
    }
}

/// How bookmarked commits without an open PR are handled when a multi-commit
/// revset is expanded by [`resolve_pr_targets`]. Commits without any bookmark
/// are always skipped: they are the interior commits of multi-commit PRs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingPr {
    /// Skip the commit with a note. For lenient batch commands (`rerun`,
    /// `auto-merge`) where "no PR here" is not actionable.
    Skip,
    /// Fail the whole command. For exact assertions (`stack`) where a
    /// bookmark that is not a PR cannot be part of the requested shape.
    Error,
}

/// Resolve a mixed list of PR numbers and jj revsets into deduped
/// [`PrDetails`], in argument order. A numeric argument is treated as a PR
/// number; anything else is a revset, which may match multiple commits.
///
/// Revsets are expanded oldest-first, so a range like `qv::uw` yields PRs
/// bottom of the stack first. Commits without a bookmark are skipped (they
/// are interior commits of multi-commit PRs); bookmarked commits without an
/// open PR follow `missing_pr`. A revset that resolves to exactly one commit
/// keeps the hard errors of single-rev commands regardless of `missing_pr`.
///
/// # Errors
///
/// Returns an error if a revset is invalid or matches no commits, on the
/// single-commit cases above, or if any underlying jj/GH call fails.
pub async fn resolve_pr_targets<J: Jj, G: Gh>(
    jj: &J,
    gh: &G,
    target: &remote::Target,
    args: &[String],
    missing_pr: MissingPr,
) -> Result<Vec<PrDetails>> {
    let mut seen = HashSet::<u64>::new();
    let mut prs = Vec::<PrDetails>::new();
    for arg in args {
        // if given a literal PR number (as opposed to a revset)
        if let Ok(number) = arg.parse::<u64>() {
            if seen.insert(number) {
                prs.push(gh.get_pr(&target.owner, &target.repo, number).await?);
            }
            continue;
        }

        let mut commits = jj.resolve_revset(arg).await?;
        if commits.is_empty() {
            bail!("revset `{arg}` matched no commits");
        }
        // jj log emits newest-first; reverse so stacks come out bottom-first.
        commits.reverse();
        let single = commits.len() == 1;
        for commit in &commits {
            let number = match_commit_to_pr(gh, target, arg, commit, single, missing_pr).await?;
            let Some(number) = number else { continue };
            if seen.insert(number) {
                prs.push(gh.get_pr(&target.owner, &target.repo, number).await?);
            }
        }
    }
    Ok(prs)
}

/// Find the open PR for one commit of a resolved revset. For a single-commit
/// revset a missing bookmark or PR is a hard error (matching the behavior of
/// other single-rev commands); in a multi-commit revset, bookmark-less
/// commits are skipped and PR-less bookmarks follow `missing_pr`.
async fn match_commit_to_pr(
    gh: &impl Gh,
    target: &remote::Target,
    arg: &str,
    commit: &CommitInfo,
    single: bool,
    missing_pr: MissingPr,
) -> Result<Option<u64>> {
    let Some(branch) = commit.bookmarks.first() else {
        if single {
            bail!("no local bookmark on `{arg}`; nothing to look up");
        }
        log::info!(
            "skipping {}: no local bookmark",
            short_id(&commit.change_id)
        );
        return Ok(None);
    };
    let head_spec = target.head_spec(branch);
    let summary = gh
        .find_open_pr(&target.owner, &target.repo, &head_spec)
        .await?;
    let Some(summary) = summary else {
        if single || missing_pr == MissingPr::Error {
            bail!("no open PR for revision `{arg}` (head `{head_spec}`)");
        }
        log::info!(
            "skipping {} (head `{head_spec}`): no open PR",
            short_id(&commit.change_id)
        );
        return Ok(None);
    };
    Ok(Some(summary.number))
}

fn short_id(id: &str) -> &str {
    &id[..id.len().min(8)]
}

/// Resolve a revision into its PR-lookup context: bookmark, remote target,
/// head spec, trunk bookmark, and any existing open PR.
///
/// # Errors
///
/// Returns an error if `rev` has no local bookmark, if the configured
/// `default_remote` is unset, if `trunk()` is empty, or if any underlying
/// jj/GH call fails.
pub async fn resolve_pr_for_rev<J: Jj, G: Gh>(
    jj: &J,
    gh: &G,
    default_remote: &str,
    upstream_remote: &str,
    rev: &str,
) -> Result<PrLookup> {
    let info = jj.resolve_rev(rev).await?;
    let branch = info
        .bookmarks
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("no local bookmark on `{rev}`; nothing to look up"))?;

    let origin_url = jj
        .remote_url(default_remote)?
        .ok_or_else(|| anyhow!("`{default_remote}` remote is not configured"))?;
    let upstream_url = jj.remote_url(upstream_remote)?;
    let target = remote::target(&origin_url, upstream_url.as_deref())?;
    let head_spec = target.head_spec(&branch);

    let default_base = jj
        .trunk_branch()
        .await?
        .ok_or_else(|| anyhow!("could not detect trunk() bookmark"))?;

    let summary = gh
        .find_open_pr(&target.owner, &target.repo, &head_spec)
        .await?;

    Ok(PrLookup {
        branch,
        target,
        head_spec,
        default_base,
        summary,
    })
}
