//! Repository-wide read-only audit, including saved Card Evidence selectors.
//!
//! Structural and reference problems are collected first. A damaged Card is
//! already reported by `check_references`; it does not prevent verification
//! of Evidence in other, readable Cards.

use digital_notes::{
    evidence::Resolution,
    model::CardId,
    repository::{IntegrityIssueKind, Repository},
};
use serde::Serialize;
use std::{error::Error, fs, io, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Serialize)]
struct CheckIssue {
    kind: &'static str,
    path: String,
    detail: String,
}

#[derive(Debug, Serialize)]
struct CheckReport {
    ok: bool,
    issue_count: usize,
    issues: Vec<CheckIssue>,
}

fn issue_kind(kind: IntegrityIssueKind) -> &'static str {
    match kind {
        IntegrityIssueKind::MissingWork => "missing_work",
        IntegrityIssueKind::MissingAsset => "missing_asset",
        IntegrityIssueKind::MissingCard => "missing_card",
        IntegrityIssueKind::OrphanAssetHtml => "orphan_asset_html",
        IntegrityIssueKind::MissingAssetHtml => "missing_asset_html",
        IntegrityIssueKind::InvalidEntry => "invalid_entry",
        IntegrityIssueKind::InvalidEntity => "invalid_entity",
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn collect_issues(repo: &Repository) -> Result<CheckReport> {
    let mut issues: Vec<CheckIssue> = repo
        .check_references()?
        .into_iter()
        .map(|issue| CheckIssue {
            kind: issue_kind(issue.kind),
            path: display_path(repo.root(), &issue.path),
            detail: issue.detail,
        })
        .collect();

    // Skip unreadable Cards: the structural audit above has already reported
    // them. Do not use list_cards(), which fails at the first malformed Card.
    let cards_dir = repo.root().join("cards");
    for entry in fs::read_dir(&cards_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(id) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<CardId>().ok())
        else {
            continue;
        };
        let Ok(card) = repo.load_card(&id) else {
            continue;
        };
        if card.value.evidence.is_empty() {
            continue;
        }

        let relative_path = display_path(repo.root(), &path);
        match repo.verify_card_evidence(&id) {
            Ok(statuses) => {
                for (evidence_id, result) in statuses {
                    let (kind, detail) = match result {
                        Resolution::Verified => continue,
                        Resolution::Recovered(start) => (
                            "evidence_position_changed",
                            format!(
                                "Evidence {evidence_id} matches at codepoint {start}, but the stored position differs"
                            ),
                        ),
                        Resolution::Unresolved => (
                            "evidence_unresolved",
                            format!("Evidence {evidence_id} cannot be verified against its Asset"),
                        ),
                    };
                    issues.push(CheckIssue {
                        kind,
                        path: relative_path.clone(),
                        detail,
                    });
                }
            }
            Err(error) => issues.push(CheckIssue {
                kind: "evidence_verification_error",
                path: relative_path,
                detail: format!("Card {id}: {error}"),
            }),
        }
    }

    issues.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.kind.cmp(b.kind))
            .then(a.detail.cmp(&b.detail))
    });
    Ok(CheckReport {
        ok: issues.is_empty(),
        issue_count: issues.len(),
        issues,
    })
}

pub fn run(repo: &Repository, json: bool) -> Result<()> {
    let report = collect_issues(repo)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if report.ok {
        println!("OK: repository integrity check passed");
    } else {
        println!("Found {} integrity issue(s):", report.issue_count);
        for issue in &report.issues {
            println!("- [{}] {}: {}", issue.kind, issue.path, issue.detail);
        }
    }
    if report.ok {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "repository integrity check failed ({} issue(s))",
            report.issue_count
        ))
        .into())
    }
}
