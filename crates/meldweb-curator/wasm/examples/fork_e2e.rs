//! Forking another player's deck (ADR-0041) from a real public host,
//! natively and with no login: what the browser does through `/api/git/`,
//! done here with ureq.
//!
//! ```text
//! cargo run -p meldweb-wasm --example fork_e2e -- \
//!   https://github.com/<owner>/<repo>.git <their deck's stem> <empty dir> [<local stem>]
//! ```
//!
//! It makes a fresh version-2 repo in the dir, fetches their deck's line
//! alone, forks it (under `<local stem>` when given), and prints the fork's
//! log and what is tracked. Run it again on the same dir after they push:
//! it fetches again, says how many commits are new, and takes them, a
//! fast-forward or a merge commit, printing which.

use std::io::Read;
use std::path::Path;
use std::process::Command;

use chip_decklist::stem::DeckStem;
use meldweb_git::{
    BranchName, Change, FsStore, Identity, Repo, RepoPath, Signature, Timestamp, Tip, WriteCommit,
};
use meldweb_wasm::branches::MergePlan;
use meldweb_wasm::preference::Upstream;
use meldweb_wasm::{layout, upstream};

type Err = Box<dyn std::error::Error>;

fn read(response: ureq::http::Response<ureq::Body>) -> Result<Vec<u8>, Err> {
    let mut bytes = Vec::new();
    response.into_body().as_reader().read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn sig() -> Signature {
    Signature {
        who: Identity::new("Progress Engine e2e", "e2e@localhost").unwrap(),
        when: Timestamp::new(now(), 0).unwrap(),
    }
}

fn fetch(repo: &mut Repo<FsStore>, up: &Upstream) -> Result<upstream::Fetched, Err> {
    let url = up.url.as_str();
    let r = ureq::get(&format!("{url}/info/refs?service=git-upload-pack"))
        .header("User-Agent", "git/progress-engine")
        .call()?;
    println!(
        "GET info/refs?service=git-upload-pack (no login): {}",
        r.status()
    );
    let ad = read(r)?;
    let response = match upstream::fetch_request(repo, up, &ad)? {
        None => {
            println!("nothing to fetch");
            None
        }
        Some(body) => {
            let r = ureq::post(&format!("{url}/git-upload-pack"))
                .header("User-Agent", "git/progress-engine")
                .header("Content-Type", "application/x-git-upload-pack-request")
                .header("Accept", "application/x-git-upload-pack-result")
                .send(&body)?;
            println!(
                "POST git-upload-pack ({} bytes): {}",
                body.len(),
                r.status()
            );
            let bytes = read(r)?;
            println!("  {} bytes back", bytes.len());
            Some(bytes)
        }
    };
    Ok(upstream::fetch_finish(repo, up, &ad, response.as_deref())?)
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

fn main() -> Result<(), Err> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [url, deck, dir, rest @ ..] = args.as_slice() else {
        return Err("usage: fork_e2e <url> <their stem> <dir> [<local stem>]".into());
    };
    let up = Upstream::new(url, deck, None)?;
    let local = DeckStem::new(rest.first().map_or(deck.as_str(), String::as_str))?;
    let dir = Path::new(dir);
    let fresh = !dir.join("HEAD").exists();
    let mut repo = if fresh {
        let mut repo = Repo::init(FsStore::new(dir))?;
        repo.commit(
            &BranchName::new("main")?,
            Tip::Unborn,
            &WriteCommit {
                changes: vec![Change::Put {
                    path: RepoPath::new("VERSION")?,
                    bytes: b"2\n".to_vec(),
                }],
                message: "init\n".into(),
                author: sig(),
                committer: sig(),
            },
        )?;
        repo
    } else {
        Repo::open(FsStore::new(dir))?
    };
    let fetched = fetch(&mut repo, &up)?;
    println!(
        "fetched {}'s {} at {} ({:?}){}",
        up.owner(),
        up.deck,
        fetched.commit,
        fetched.name,
        if fetched.migrated {
            ", migrated from version 1 here"
        } else {
            ""
        }
    );
    if fresh {
        let forked = upstream::fork(&mut repo, &up, &local, &sig())?;
        println!("forked: {forked:?}");
    } else {
        let f = upstream::following(&repo, &local)?.ok_or("the deck follows nothing")?;
        let theirs = f.theirs.ok_or("nothing fetched")?;
        println!(
            "{} new on {}'s {}, {} of mine",
            theirs.new,
            up.owner(),
            up.deck,
            theirs.ahead
        );
        let plan = upstream::plan(&mut repo, &local, None)?;
        let (into, from) = match &plan {
            MergePlan::FastForward { into, from, .. } => {
                println!("plan: fast-forward");
                (into.clone(), from.clone())
            }
            MergePlan::Merge {
                into,
                from,
                message,
                ..
            } => {
                println!("plan: merge, {message}");
                (into.clone(), from.clone())
            }
            other => {
                println!("plan: {other:?}");
                return Ok(());
            }
        };
        let done = upstream::take(&mut repo, &local, &into, &from, None, &sig())?;
        println!("took: {done:?}");
    }
    let line = layout::main_line(&local);
    println!(
        "{}",
        git(dir, &["log", "--format=%h %an: %s", line.as_str()])
    );
    println!(
        "{}",
        git(
            dir,
            &["for-each-ref", "--format=%(objectname:short) %(refname)"]
        )
    );
    println!("{}", git(dir, &["fsck", "--strict", "--no-dangling"]));
    Ok(())
}
