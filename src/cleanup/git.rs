//! git, read-only and hardened (design D-8), with a time limit on every call.
//!
//! Agents work in these repos while Tidegauge reads them, so `status` must
//! never take `index.lock` (`GIT_OPTIONAL_LOCKS=0`), and a repo's own config
//! must not make git run a program (fsmonitor, hooks). Tidegauge never
//! fetches.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The git to run, or none. `/usr/bin/git` is only a shim that offers to
/// install the Command Line Tools when they are missing, so it is used only
/// when a developer folder exists.
pub fn executable() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let developer = Path::new("/Library/Developer/CommandLineTools/usr/bin/git").exists()
                || Path::new("/Applications/Xcode.app/Contents/Developer/usr/bin/git").exists();
            if developer {
                return Some(PathBuf::from("/usr/bin/git"));
            }
            ["/opt/homebrew/bin/git", "/usr/local/bin/git"]
                .into_iter()
                .map(PathBuf::from)
                .find(|p| p.exists())
        })
        .as_deref()
}

/// A git command in `dir` with the hardened environment.
pub fn command(dir: &Path) -> Option<Command> {
    let mut c = Command::new(executable()?);
    c.arg("-C")
        .arg(dir)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "color.ui=false",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_CEILING_DIRECTORIES")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .current_dir("/");
    Some(c)
}

/// Exit code and standard output, or none when git could not run or timed out.
pub fn run_status(dir: &Path, args: &[&str]) -> Option<(i32, String)> {
    let mut c = command(dir)?;
    c.args(args);
    let out = crate::packages::run_command(c, TIMEOUT).ok()?;
    Some((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// How long a write (`worktree remove`, `prune`) may take: removing a
/// worktree full of build output can take minutes, and killing git halfway
/// would leave it half deleted.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(600);

/// A write call: exit code 0 within `WRITE_TIMEOUT`.
pub fn write(dir: &Path, args: &[&str]) -> bool {
    let Some(mut c) = command(dir) else { return false };
    c.args(args);
    crate::packages::run_command(c, WRITE_TIMEOUT).is_ok_and(|o| o.status.success())
}

/// Standard output when git exits 0.
pub fn run(dir: &Path, args: &[&str]) -> Option<String> {
    match run_status(dir, args)? {
        (0, out) => Some(out),
        _ => None,
    }
}

/// One record of `git worktree list --porcelain`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listed {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub prunable: bool,
    pub bare: bool,
}

pub fn parse_worktree_list(text: &str) -> Vec<Listed> {
    let mut out = Vec::new();
    let mut cur: Option<Listed> = None;
    for line in text.lines() {
        if line.is_empty() {
            out.extend(cur.take());
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if key == "worktree" {
            out.extend(cur.take());
            cur = Some(Listed { path: PathBuf::from(value), ..Default::default() });
            continue;
        }
        let Some(w) = cur.as_mut() else { continue };
        match key {
            "HEAD" => w.head = Some(value.to_string()),
            "branch" => {
                w.branch = Some(value.strip_prefix("refs/heads/").unwrap_or(value).to_string())
            }
            "detached" => w.detached = true,
            "locked" => w.locked = true,
            "prunable" => w.prunable = true,
            "bare" => w.bare = true,
            _ => {}
        }
    }
    out.extend(cur);
    out
}

/// `git status --porcelain=v1 --ignored=matching`, split into changed lines
/// and ignored paths.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub changed: Vec<String>,
    pub ignored: Vec<String>,
}

pub fn parse_status(text: &str) -> Status {
    let mut s = Status::default();
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("!! ") {
            s.ignored.push(path.trim_matches('"').to_string());
        } else if line.len() > 3 {
            s.changed.push(line.to_string());
        }
    }
    s
}

pub fn status(dir: &Path) -> Option<Status> {
    run(
        dir,
        &["status", "--porcelain=v1", "--untracked-files=normal", "--ignored=matching"],
    )
    .map(|t| parse_status(&t))
}

pub fn head(dir: &Path) -> Option<String> {
    run(dir, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string())
}

/// The branch merges are judged against: origin's default, else main or master.
pub fn base(repo: &Path) -> Option<String> {
    if let Some(b) = run(repo, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]) {
        let b = b.trim();
        if !b.is_empty() {
            return Some(b.to_string());
        }
    }
    ["origin/main", "origin/master", "main", "master"]
        .into_iter()
        .find(|r| run(repo, &["rev-parse", "--verify", "--quiet", r]).is_some())
        .map(str::to_string)
}

pub fn is_ancestor(dir: &Path, commit: &str, of: &str) -> Option<bool> {
    match run_status(dir, &["merge-base", "--is-ancestor", commit, of])? {
        (0, _) => Some(true),
        (1, _) => Some(false),
        _ => None,
    }
}

/// Commits on HEAD that its upstream lacks; none when there is no upstream.
pub fn unpushed(dir: &Path) -> Option<u32> {
    run(dir, &["rev-list", "--count", "@{u}..HEAD"]).and_then(|s| s.trim().parse().ok())
}

pub fn last_commit(dir: &Path) -> Option<i64> {
    run(dir, &["log", "-1", "--format=%ct"]).and_then(|s| s.trim().parse().ok())
}

/// Whether `rel` (inside `repo`) is ignored by git.
pub fn is_ignored(repo: &Path, rel: &str) -> Option<bool> {
    match run_status(repo, &["check-ignore", "-q", "--", rel])? {
        (0, _) => Some(true),
        (1, _) => Some(false),
        _ => None,
    }
}

/// Whether git tracks any file under `rel`.
pub fn tracks_any(repo: &Path, rel: &str) -> Option<bool> {
    run(repo, &["ls-files", "--", rel]).map(|s| !s.trim().is_empty())
}

/// The main repo a linked worktree's `.git` file points to, and whether that
/// record still exists. None when `.git` is not a worktree link.
pub fn link_target(worktree: &Path) -> Option<(PathBuf, bool)> {
    let meta = std::fs::symlink_metadata(worktree.join(".git")).ok()?;
    if !meta.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(worktree.join(".git")).ok()?;
    let gitdir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
    let gitdir = if gitdir.is_absolute() { gitdir } else { worktree.join(gitdir) };
    // <main>/.git/worktrees/<name>
    let main = gitdir.parent()?.parent()?.parent()?.to_path_buf();
    Some((main, gitdir.exists()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_worktree_list() {
        let text = "worktree /r\nHEAD aaa\nbranch refs/heads/main\n\nworktree /r/.claude/worktrees/x\nHEAD bbb\nbranch refs/heads/claude/x\nlocked\n\nworktree /gone\nHEAD ccc\ndetached\nprunable gitdir file points to non-existent location\n";
        let l = parse_worktree_list(text);
        assert_eq!(l.len(), 3);
        assert_eq!(l[1].branch.as_deref(), Some("claude/x"));
        assert!(l[1].locked);
        assert!(l[2].detached && l[2].prunable);
    }

    #[test]
    fn parses_status() {
        let s = parse_status(" M a.rs\n?? new.txt\n!! node_modules/\n!! .env\n");
        assert_eq!(s.changed.len(), 2);
        assert_eq!(s.ignored, vec!["node_modules/", ".env"]);
    }
}
