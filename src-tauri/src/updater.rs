//! Following nxnative's main branch (docs/frontends.md, "Following main"):
//! nxnative has no releases, so the launcher pins a commit and moves it
//! forward. Between the pinned commit A and the new one B, git says what
//! changed for a front end, and commit lines `Front-ends: break|interface|new:`
//! say what only the author knows.

use crate::engine::{child_path, installed_metal, metal_in_lock, Engine};
use crate::library::Game;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

const MAIN: &str = "origin/main";
const MAX_COMMITS: usize = 150;
const MAX_DIFF: usize = 12_000;

async fn git_out(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("PATH", child_path(std::env::var("PATH").ok().as_deref()))
        .output()
        .await
        .map_err(|e| format!("git non si avvia: {e}"))
}

/// Runs git in the engine checkout; its stdout, or its stderr as the error.
async fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_out(root, args).await?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Whether anything under `paths` differs between two commits.
async fn changed(root: &Path, a: &str, b: &str, paths: &[&str]) -> bool {
    let mut args = vec!["diff", "--quiet", a, b, "--"];
    args.extend_from_slice(paths);
    git_out(root, &args).await.is_ok_and(|o| o.status.code() == Some(1))
}

pub async fn head(root: &Path) -> Result<String, String> {
    git(root, &["rev-parse", "HEAD"]).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub commit: String,
    pub short: String,
    pub date: String,
    pub subject: String,
    /// Local changes in tracked files: an update would not be clean.
    pub modified: bool,
}

pub async fn info(engine: &Engine) -> Result<EngineInfo, String> {
    let line = git(&engine.root, &["log", "-1", "--format=%H%x1f%h%x1f%ad%x1f%s", "--date=short"]).await?;
    let f: Vec<&str> = line.splitn(4, '\x1f').collect();
    let modified = !git(&engine.root, &["status", "--porcelain", "--untracked-files=no"]).await?.is_empty();
    Ok(EngineInfo {
        commit: f[0].into(),
        short: f.get(1).copied().unwrap_or_default().into(),
        date: f.get(2).copied().unwrap_or_default().into(),
        subject: f.get(3).copied().unwrap_or_default().into(),
        modified,
    })
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub commit: String,
    /// "break", "interface", "new", or "note" for a line without a kind.
    pub kind: String,
    pub text: String,
}

/// The `Front-ends:` lines of `git log --format=%x1e%h%x1f%B`.
pub fn parse_notes(log: &str) -> Vec<Note> {
    log.split('\x1e')
        .filter_map(|rec| rec.split_once('\x1f'))
        .flat_map(|(commit, body)| {
            body.lines().filter_map(move |l| {
                let rest = l.trim().strip_prefix("Front-ends:")?.trim();
                let (kind, text) = match rest.split_once(':') {
                    Some((k, t)) if ["break", "interface", "new"].contains(&k.trim()) => (k.trim(), t.trim()),
                    _ => ("note", rest),
                };
                Some(Note { commit: commit.trim().into(), kind: kind.into(), text: text.into() })
            })
        })
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitLine {
    pub short: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub from: String,
    pub to: String,
    pub to_short: String,
    pub up_to_date: bool,
    /// The pinned commit is not on main (a local or older branch): moving
    /// would leave its commits behind.
    pub diverged: bool,
    pub total_commits: usize,
    pub commits: Vec<CommitLine>,
    pub notes: Vec<Note>,
    // What git shows (the table in "Following main").
    pub frontends_doc: bool,
    pub schema_diff: Option<String>,
    pub settings_list: bool,
    pub script_options: Vec<String>,
    pub env_switches: bool,
    /// The shader chain changed: every game rebuilds its shaders at its next build.
    pub shader_chain: bool,
    /// The Python requirements changed: the update reinstalls them.
    pub requirements: bool,
    /// The Metal compiler B is locked to, when it is not the one installed.
    pub metal_needed: Option<String>,
}

/// Fetches main and says what moving to it would change.
pub async fn check(engine: &Engine) -> Result<UpdateInfo, String> {
    let root = &engine.root;
    let fetch = tokio::time::timeout(Duration::from_secs(90), git(root, &["fetch", "--quiet", "origin", "main"])).await;
    match fetch {
        Err(_) => return Err("GitHub non risponde (90 s): riprova più tardi".into()),
        Ok(Err(e)) => return Err(format!("Impossibile scaricare gli aggiornamenti: {e}")),
        Ok(Ok(_)) => {}
    }
    let a = head(root).await?;
    let b = git(root, &["rev-parse", MAIN]).await?;
    let to_short = git(root, &["rev-parse", "--short", &b]).await?;
    let range = format!("{a}..{b}");
    let diverged = git_out(root, &["merge-base", "--is-ancestor", &a, &b]).await.is_ok_and(|o| !o.status.success());

    let total_commits = git(root, &["rev-list", "--count", &range]).await?.parse().unwrap_or(0);
    let commits = git(root, &["log", "--format=%h%x1f%ad%x1f%s", "--date=short", &format!("-{MAX_COMMITS}"), &range])
        .await?
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(3, '\x1f').collect();
            (f.len() == 3).then(|| CommitLine { short: f[0].into(), date: f[1].into(), subject: f[2].into() })
        })
        .collect();
    let notes = parse_notes(&git(root, &["log", "--format=%x1e%h%x1f%B", "--grep=^Front-ends:", &range]).await?);

    let schema_diff = if changed(root, &a, &b, &["docs/launcher.schema.json"]).await {
        let mut d = git(root, &["diff", &a, &b, "--", "docs/launcher.schema.json"]).await?;
        if d.len() > MAX_DIFF {
            d.truncate(d.floor_char_boundary(MAX_DIFF));
            d.push_str("\n…");
        }
        Some(d)
    } else {
        None
    };
    let script_options = git(root, &["diff", &a, &b, "--", "scripts/build_ipa_from_nsp.py"])
        .await?
        .lines()
        .filter(|l| (l.starts_with('+') || l.starts_with('-')) && l.contains("add_argument("))
        .map(|l| l.chars().take(220).collect())
        .collect();
    let chain = git(root, &["log", "--oneline", &range, "--", "dm/gfx/nvn/dmsh_chain.h"]).await?;
    let lock_b = git(root, &["show", &format!("{b}:dm/gfx/tools/dmsh_chain.lock")]).await.unwrap_or_default();
    let metal_b = metal_in_lock(&lock_b);
    let metal_needed = match (metal_b, installed_metal().await) {
        (Some(need), Some(have)) if need != have => Some(need),
        (Some(need), None) => Some(need),
        _ => None,
    };

    Ok(UpdateInfo {
        up_to_date: a == b,
        frontends_doc: changed(root, &a, &b, &["docs/frontends.md"]).await,
        settings_list: changed(root, &a, &b, &["dm/games/settings.tsv"]).await,
        env_switches: changed(root, &a, &b, &["docs/env_switches.tsv"]).await,
        requirements: changed(root, &a, &b, &[".github/requirements/tools.txt"]).await,
        shader_chain: !chain.is_empty(),
        from: a,
        to: b,
        to_short,
        diverged,
        total_commits,
        commits,
        notes,
        schema_diff,
        script_options,
        metal_needed,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    #[serde(flatten)]
    pub info: EngineInfo,
    pub previous: String,
    /// Output of the requirements' reinstall, when they changed.
    pub pip: Option<String>,
}

/// Moves the engine to `target` (main's commit, or the previous one for a
/// rollback): a detached checkout, then the Python requirements when they
/// differ. The launcher's repo then shows the submodule as changed; a
/// commit there pins the new version.
pub async fn apply(engine: &Engine, target: &str) -> Result<Applied, String> {
    let root = &engine.root;
    if info(engine).await?.modified {
        return Err("Il motore ha modifiche locali: l'aggiornamento le sovrascriverebbe".into());
    }
    let previous = head(root).await?;
    let requirements = changed(root, &previous, target, &[".github/requirements/tools.txt"]).await;
    git(root, &["checkout", "--quiet", "--detach", target]).await.map_err(|e| format!("Checkout fallito: {e}"))?;

    let pip = if requirements {
        let req = root.join(".github/requirements/tools.txt");
        let out = Command::new(engine.pip())
            .args(["install", "--disable-pip-version-check", "-q", "-r"])
            .arg(&req)
            .env("PATH", child_path(std::env::var("PATH").ok().as_deref()))
            .output()
            .await
            .map_err(|e| format!("pip non si avvia: {e}"))?;
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        if !out.status.success() {
            return Err(format!("Motore aggiornato, ma l'installazione delle dipendenze Python è fallita:\n{text}"));
        }
        Some(if text.trim().is_empty() { "Dipendenze Python aggiornate".into() } else { text })
    } else {
        None
    };
    Ok(Applied { info: info(engine).await?, previous, pip })
}

/// Why a game built with an older engine is worth building again. Only games
/// this launcher built know their engine commit; others get no hint.
pub async fn rebuild_hints(engine: &Engine, games: &[Game]) -> HashMap<String, Vec<String>> {
    let root = &engine.root;
    let Ok(now) = head(root).await else { return HashMap::new() };
    let mut hints = HashMap::new();
    for g in games {
        let Some(built) = g.built_with.as_deref().filter(|c| *c != now) else { continue };
        if git(root, &["cat-file", "-e", &format!("{built}^{{commit}}")]).await.is_err() {
            hints.insert(g.title_id.clone(), vec!["Costruito con una versione del motore che qui non c'è più".into()]);
            continue;
        }
        // Built with a newer engine (after a rollback) or another branch: the
        // differences are not "fixes since your build", so say only that.
        if !git_out(root, &["merge-base", "--is-ancestor", built, &now]).await.is_ok_and(|o| o.status.success()) {
            hints.insert(
                g.title_id.clone(),
                vec!["Costruito con un'altra versione del motore (più recente, o di un altro ramo)".into()],
            );
            continue;
        }
        let mut why = Vec::new();
        if changed(root, built, &now, &[&format!("dm/games/{}", g.title_id)]).await {
            why.push("Il profilo di questo gioco è cambiato".to_string());
        }
        if !git(root, &["log", "--oneline", &format!("{built}..{now}"), "--", "dm/gfx/nvn/dmsh_chain.h"])
            .await
            .unwrap_or_default()
            .is_empty()
        {
            why.push("Nuova catena di shader: la prossima build le ricostruisce".into());
        }
        if changed(root, built, &now, &["dm", "scripts"]).await {
            why.push("Il motore ha correzioni successive alla tua build".into());
        }
        if !why.is_empty() {
            hints.insert(g.title_id.clone(), why);
        }
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_front_end_notes() {
        let log = "\x1e5714e32\x1fSubject line\n\nBody text.\nFront-ends: new: --fallback-icon IMAGE\n\
                   Front-ends: break: something stopped working\n\
                   \x1eab12cd3\x1fOther\n\nFront-ends: the shader chain version changes; shaders are rebuilt.\n\
                   Front-ends: interface: a field now means X: really";
        let notes = parse_notes(log);
        let got: Vec<_> = notes.iter().map(|n| (n.commit.as_str(), n.kind.as_str(), n.text.as_str())).collect();
        assert_eq!(
            got,
            [
                ("5714e32", "new", "--fallback-icon IMAGE"),
                ("5714e32", "break", "something stopped working"),
                ("ab12cd3", "note", "the shader chain version changes; shaders are rebuilt."),
                ("ab12cd3", "interface", "a field now means X: really"),
            ]
        );
    }

    #[test]
    fn applied_is_flat_for_the_ui() {
        // The UI reads applied.short and applied.date at the top level.
        let a = Applied {
            info: EngineInfo { commit: "f75ac06e85".into(), short: "f75ac06e".into(), date: "2026-11-11".into(), subject: "s".into(), modified: false },
            previous: "f6465454".into(),
            pip: None,
        };
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!((v["short"].as_str(), v["date"].as_str()), (Some("f75ac06e"), Some("2026-11-11")));
    }

    #[test]
    fn reads_the_metal_lock() {
        let lock = "# comment\ndmsh_xlate\tb59d\nmetal_macosx\tApple metal version 32023.922 (metalfe-32023.922.4)\n";
        assert_eq!(metal_in_lock(lock).as_deref(), Some("Apple metal version 32023.922 (metalfe-32023.922.4)"));
    }
}
