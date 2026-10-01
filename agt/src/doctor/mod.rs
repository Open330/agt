//! `agt doctor`: offline health checks for every installed skill, whoever
//! installed it. Design: docs/design/0001-agent-env-manifest-and-doctor.md §10

pub mod checks;
pub mod scan;

use crate::environment::lock::Lockfile;
use crate::environment::manifest::Scope;
use crate::ui;
use anyhow::Result;
use checks::{Finding, ManagedScope, Options, Severity};
use colored::Colorize;
use scan::ScopeKind;

pub fn execute(json: bool, budget: usize, no_gh: bool) -> Result<()> {
    let mut skills = scan::scan(&scan::default_roots());
    if !no_gh {
        let extra = scan::gh_hosts(&skills);
        skills.extend(extra);
    }

    let mut scopes = Vec::new();
    if crate::config::git_root().is_some() {
        scopes.push(managed_scope(Scope::project()?, ScopeKind::Project)?);
    }
    scopes.push(managed_scope(Scope::user()?, ScopeKind::User)?);

    let findings = checks::run(&skills, &scopes, &Options { budget });
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();

    if json {
        let report = serde_json::json!({ "skills": skills, "findings": findings });
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print(&findings, skills.len());
    }
    if errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn managed_scope(scope: Scope, kind: ScopeKind) -> Result<ManagedScope> {
    Ok(ManagedScope {
        kind,
        owner: scope.owner(),
        flag: scope.flag(),
        has_manifest: scope.manifest_path().is_file(),
        lock: Lockfile::load(&scope.lock_path())?,
    })
}

fn print(findings: &[Finding], scanned: usize) {
    ui::section(&format!("agt doctor — {scanned} skills scanned"));
    for f in findings {
        let mark = match f.severity {
            Severity::Error => "✗".red(),
            Severity::Warn => "⚠".yellow(),
            Severity::Info => "·".dimmed(),
        };
        let subject = f
            .skill
            .as_deref()
            .map(|s| format!("{}: ", s.bold()))
            .unwrap_or_default();
        println!("  {mark} {} {subject}{}", f.id.dimmed(), f.message);
        if let Some(path) = &f.path {
            println!("       {}", path.display().to_string().dimmed());
        }
        if let Some(hint) = &f.hint {
            println!("       → {hint}");
        }
    }
    let count = |s| findings.iter().filter(|f| f.severity == s).count();
    let (errors, warns) = (count(Severity::Error), count(Severity::Warn));
    println!();
    if errors + warns == 0 {
        ui::success("No problems found");
    } else {
        ui::info(&format!("{errors} error(s), {warns} warning(s)"));
    }
}
