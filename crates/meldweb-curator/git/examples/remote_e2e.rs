//! The remote protocol against a real host, natively, with no worker between:
//! what the browser does through `/api/git/`, done here with ureq.
//!
//! ```text
//! cargo run -p meldweb-git --example remote_e2e -- \
//!   https://github.com/<owner>/<repo>.git "$(gh auth token)" <empty dir> [<a clone of it>]
//! ```
//!
//! It makes a fresh repo in the dir, adopts the remote's history, commits an
//! edit and a snapshot tag, and pushes them. With a clone, it then pushes a
//! commit from the clone between reading the remote's refs and sending its
//! own push, so the host refuses that push, and recovers by fetching,
//! merging and pushing again. Each step prints what it came to.

use std::io::Read;
use std::path::Path;
use std::process::Command;

use meldweb_git::{
    merge_lines, BranchName, Change, FetchRequest, FsStore, Identity, PushRequest, RemoteUrl, Repo,
    RepoPath, Signature, TagName, Timestamp, Tip, WriteCommit,
};

type Err = Box<dyn std::error::Error>;

struct Host {
    url: String,
    auth: String,
}

impl Host {
    fn read(response: ureq::http::Response<ureq::Body>) -> Result<Vec<u8>, Err> {
        let mut bytes = Vec::new();
        response.into_body().as_reader().read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn advertise(&self, service: &str) -> Result<Vec<u8>, Err> {
        let r = ureq::get(&format!("{}/info/refs?service={service}", self.url))
            .header("Authorization", &self.auth)
            .header("User-Agent", "git/progress-engine")
            .call()?;
        let kind = r.headers().get("content-type").cloned();
        println!("GET info/refs?service={service}: {} {kind:?}", r.status());
        Self::read(r)
    }

    fn post(&self, service: &str, body: &[u8]) -> Result<Vec<u8>, Err> {
        let r = ureq::post(&format!("{}/{service}", self.url))
            .header("Authorization", &self.auth)
            .header("User-Agent", "git/progress-engine")
            .header("Content-Type", &format!("application/x-{service}-request"))
            .header("Accept", &format!("application/x-{service}-result"))
            .send(body)?;
        println!("POST {service} ({} bytes): {}", body.len(), r.status());
        Self::read(r)
    }
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

fn by_lines(_: &str, base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    let s = |b: &[u8]| String::from_utf8(b.to_vec()).ok();
    merge_lines(&s(base)?, &s(ours)?, &s(theirs)?).map(String::into_bytes)
}

fn fetch(repo: &mut Repo<FsStore>, host: &Host) -> Result<(), Err> {
    let ad = host.advertise("git-upload-pack")?;
    match repo.fetch_request(&ad)? {
        FetchRequest::HaveAll => repo.fetch_finish(&ad, None)?,
        FetchRequest::Want(body) => {
            let response = host.post("git-upload-pack", &body)?;
            repo.fetch_finish(&ad, Some(&response))?;
        }
    }
    let outcome = repo.integrate(&sig(), None, &mut by_lines)?;
    println!("integrate: {outcome:?}");
    Ok(())
}

fn push(repo: &mut Repo<FsStore>, host: &Host, between: impl FnOnce()) -> Result<bool, Err> {
    let ad = host.advertise("git-receive-pack")?;
    let body = match repo.push_request(&ad)? {
        PushRequest::Send(body) => body,
        other => {
            println!("push: {other:?}");
            return Ok(false);
        }
    };
    between();
    let response = host.post("git-receive-pack", &body)?;
    let pushed = repo.push_finish(&body, &response)?;
    println!("push: {pushed:?}");
    Ok(pushed.rejected.is_empty())
}

fn edit(repo: &mut Repo<FsStore>, path: &str, line: &str) -> Result<(), Err> {
    let main = BranchName::new("main")?;
    let tip = repo.branch_tip(&main)?.expect("adopted");
    let deck = RepoPath::new(path)?;
    let mut text = String::from_utf8(repo.read_file(&tip, &deck)?.unwrap_or_default())?;
    text.push_str(line);
    text.push('\n');
    let id = repo.commit(
        &main,
        Tip::At(tip),
        &WriteCommit {
            changes: vec![Change::Put {
                path: deck,
                bytes: text.into_bytes(),
            }],
            message: format!("{path}: {line}\n"),
            author: sig(),
            committer: sig(),
        },
    )?;
    println!("committed {id}");
    Ok(())
}

fn main() -> Result<(), Err> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [url, token, dir, rest @ ..] = args.as_slice() else {
        return Err("usage: remote_e2e <https url> <token> <empty dir> [<clone>]".into());
    };
    use base64_lite as b64;
    let host = Host {
        url: url.trim_end_matches('/').to_owned(),
        // github.com takes a token as the password of HTTP basic auth,
        // with `x-access-token` as the user.
        auth: format!(
            "Basic {}",
            b64::encode(format!("x-access-token:{token}").as_bytes())
        ),
    };
    let mut repo = Repo::init(FsStore::new(dir))?;
    repo.set_remote(&RemoteUrl::new(url.as_str())?)?;
    let main = BranchName::new("main")?;
    repo.commit(
        &main,
        Tip::Unborn,
        &WriteCommit {
            changes: vec![Change::Put {
                path: RepoPath::new("VERSION")?,
                bytes: b"1\n".to_vec(),
            }],
            message: "curator: this repo is at version 1\n".to_owned(),
            author: sig(),
            committer: sig(),
        },
    )?;

    fetch(&mut repo, &host)?;
    edit(
        &mut repo,
        "decks/scratch.deck.toml",
        "# pushed by remote_e2e",
    )?;
    let tip = repo.branch_tip(&main)?.expect("committed");
    repo.create_tag(
        &TagName::new(format!("decks/scratch/e2e-{}", now()))?,
        &tip,
        &sig(),
        "pushed by remote_e2e\n",
    )?;
    println!("status before push: {:?}", repo.remote_status()?);
    push(&mut repo, &host, || {})?;
    println!("status after push: {:?}", repo.remote_status()?);

    if let [clone] = rest {
        edit(&mut repo, "decks/scratch.deck.toml", "# second, raced")?;
        let clone = Path::new(clone);
        let refused = !push(&mut repo, &host, || {
            let run = |args: &[&str]| {
                let ok = Command::new("git")
                    .arg("-C")
                    .arg(clone)
                    .args(args)
                    .status()
                    .unwrap();
                assert!(ok.success(), "git {args:?}");
            };
            run(&["pull", "-q", "--ff-only"]);
            std::fs::write(clone.join("collection.toml"), "# from the clone\n").unwrap();
            run(&["add", "collection.toml"]);
            run(&["commit", "-q", "-m", "collection: from the clone"]);
            run(&["push", "-q"]);
            println!("the clone pushed between the advertisement and the push");
        })?;
        assert!(refused, "the host must refuse a push whose old id is stale");
        fetch(&mut repo, &host)?;
        assert!(
            push(&mut repo, &host, || {})?,
            "the push after merging lands"
        );
        println!("status at the end: {:?}", repo.remote_status()?);
    }
    Ok(())
}

/// Standard base64, for the one header that needs it.
mod base64_lite {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }
}
