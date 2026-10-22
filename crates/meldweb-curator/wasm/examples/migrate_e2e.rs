//! Repo version 2's migration (ADR-0041) against a real host, natively:
//! what the browser does as it opens a version-1 repo and syncs it, done here
//! with ureq and no worker.
//!
//! ```text
//! cargo run -p meldweb-wasm --example migrate_e2e -- \
//!   https://github.com/<owner>/<repo>.git "$(gh auth token)" <empty dir>
//! ```
//!
//! It writes a version-1 repo as months of Curator saves leave one (three
//! decks edited over time, one a variant of another, snapshots, a deleted
//! deck, collection deck places and a deck order) into `<dir>/v1.git`, and
//! makes the remote exactly that with `git push --force --mirror`. Then, as
//! the browser: it migrates the repo, fetches, integrates (the remote is
//! still on version 1, so it is migrated here too, to the same commits) and
//! pushes. Each step prints what it came to; `git clone` shows the result.
//!
//! With only a dir, it writes the version-1 repo there and stops: a bare
//! repo of loose objects that zips into something the browser imports.

use std::io::Read;
use std::path::Path;
use std::process::Command;

use meldweb_git::{
    merge_lines, BranchName, Change, CommitId, FetchRequest, FsStore, Identity, PushRequest,
    RemoteUrl, Repo, RepoPath, Signature, TagName, Timestamp, Tip, WriteCommit,
};
use meldweb_wasm::layout;

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
        println!("GET info/refs?service={service}: {}", r.status());
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

fn sig(name: &str, seconds: i64, offset: i32) -> Signature {
    Signature {
        who: Identity::new(name, format!("{}@example.com", name.to_lowercase())).unwrap(),
        when: Timestamp::new(seconds, offset).unwrap(),
    }
}

fn save(
    repo: &mut Repo<FsStore>,
    puts: &[(&str, &str)],
    deletes: &[&str],
    message: &str,
    who: &Signature,
) -> Result<CommitId, Err> {
    let main = BranchName::new("main")?;
    let tip = repo.branch_tip(&main)?;
    let mut changes: Vec<Change> = puts
        .iter()
        .map(|(p, t)| {
            Ok(Change::Put {
                path: RepoPath::new(*p)?,
                bytes: t.as_bytes().to_vec(),
            })
        })
        .collect::<Result<_, Err>>()?;
    for p in deletes {
        changes.push(Change::Delete {
            path: RepoPath::new(*p)?,
        });
    }
    Ok(repo.commit(
        &main,
        tip.map_or(Tip::Unborn, Tip::At),
        &WriteCommit {
            changes,
            message: format!("{message}\n"),
            author: who.clone(),
            committer: who.clone(),
        },
    )?)
}

fn deck(name: &str, cards: &[&str], variant_of: Option<&str>) -> String {
    let mut s = format!("name = \"{name}\"\nformat = \"commander\"\n");
    if let Some(of) = variant_of {
        s.push_str(&format!("variant_of = \"decks/{of}.deck.toml\"\n"));
    }
    s.push_str("cards = [\n");
    for c in cards {
        s.push_str(&format!("  {{ name = \"{c}\" }},\n"));
    }
    s.push_str("]\n");
    s
}

/// A version-1 repo as Curator left one.
fn v1(dir: &Path) -> Result<Repo<FsStore>, Err> {
    let mut repo = Repo::init(FsStore::new(dir))?;
    let alex = |t| sig("Alex", t, 120);
    let sam = |t| sig("Sam", t, -300);
    save(
        &mut repo,
        &[("VERSION", "1\n")],
        &[],
        "curator: this repo is at version 1",
        &alex(1_760_000_000),
    )?;
    save(
        &mut repo,
        &[(
            "decks/lantern.deck.toml",
            &deck("Lantern", &["Sol Ring"], None),
        )],
        &[],
        "lantern: +1 Sol Ring",
        &alex(1_760_100_000),
    )?;
    save(
        &mut repo,
        &[("decks/old.deck.toml", &deck("Old", &["Island"], None))],
        &[],
        "old: +1 Island",
        &alex(1_760_150_000),
    )?;
    save(
        &mut repo,
        &[
            ("decks/loam.deck.toml", &deck("Loam", &["Life from the Loam"], None)),
            (
                "collection.toml",
                "cards = [\n  { name = \"Sol Ring\", at = \"Lantern\" },\n]\n\n[places]\nBulk = {}\nLantern = { deck = \"decks/lantern.deck.toml\" }\n",
            ),
        ],
        &[],
        "loam: +1 Life from the Loam",
        &sam(1_760_200_000),
    )?;
    let fnm = save(
        &mut repo,
        &[(
            "decks/lantern.deck.toml",
            &deck("Lantern", &["Sol Ring", "Mind Stone"], None),
        )],
        &[],
        "lantern: +1 Mind Stone",
        &alex(1_760_300_000),
    )?;
    save(
        &mut repo,
        &[],
        &["decks/old.deck.toml"],
        "old: delete",
        &alex(1_760_350_000),
    )?;
    let budget = save(
        &mut repo,
        &[(
            "decks/lantern-budget.deck.toml",
            &deck(
                "Lantern (budget)",
                &["Sol Ring", "Mind Stone"],
                Some("lantern"),
            ),
        )],
        &[],
        "lantern-budget: +1 Sol Ring, +1 Mind Stone",
        &alex(1_760_400_000),
    )?;
    save(
        &mut repo,
        &[
            (
                "decks/lantern.deck.toml",
                &deck(
                    "Lantern",
                    &["Sol Ring", "Mind Stone", "Arcane Signet"],
                    None,
                ),
            ),
            (
                "decks/loam.deck.toml",
                &deck("Loam", &["Life from the Loam", "Sol Ring"], None),
            ),
        ],
        &[],
        "sync: merge origin/main",
        &sam(1_760_500_000),
    )?;
    let settings = save(
        &mut repo,
        &[(
            "meldweb.toml",
            "[decks]\norder = [\n  \"decks/loam.deck.toml\",\n  \"decks/lantern.deck.toml\",\n]\n",
        )],
        &[],
        "meldweb.toml: move loam to 1 of 3",
        &alex(1_760_600_000),
    )?;
    save(
        &mut repo,
        &[(
            "decks/lantern-budget.deck.toml",
            &deck(
                "Lantern (budget)",
                &["Sol Ring", "Mind Stone", "Wayfarer's Bauble"],
                Some("lantern"),
            ),
        )],
        &[],
        "lantern-budget: +1 Wayfarer's Bauble",
        &alex(1_760_700_000),
    )?;
    let tag = |repo: &mut Repo<FsStore>, name: &str, at: CommitId, label: &str, t| {
        repo.create_tag(
            &TagName::new(name).unwrap(),
            &at,
            &alex(t),
            &format!("{label}\n"),
        )
        .map(drop)
    };
    tag(
        &mut repo,
        "decks/lantern/fnm",
        fnm,
        "FNM 14 Sep",
        1_760_310_000,
    )?;
    tag(
        &mut repo,
        "decks/loam/gp",
        settings,
        "GP Lille",
        1_760_610_000,
    )?;
    tag(
        &mut repo,
        "decks/lantern-budget/first",
        budget,
        "First",
        1_760_410_000,
    )?;
    Ok(repo)
}

fn by_lines(_: &str, base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    let s = |b: &[u8]| String::from_utf8(b.to_vec()).ok();
    merge_lines(&s(base)?, &s(ours)?, &s(theirs)?).map(String::into_bytes)
}

fn main() -> Result<(), Err> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [dir] = args.as_slice() {
        v1(Path::new(dir))?;
        println!("wrote a version-1 repo to {dir}");
        return Ok(());
    }
    let [url, token, dir] = args.as_slice() else {
        return Err("usage: migrate_e2e [<https url> <token>] <empty dir>".into());
    };
    let auth = format!(
        "Basic {}",
        base64_lite::encode(format!("x-access-token:{token}").as_bytes())
    );
    let host = Host {
        url: url.trim_end_matches('/').to_owned(),
        auth: auth.clone(),
    };
    let gitdir = Path::new(dir).join("v1.git");
    let mut repo = v1(&gitdir)?;
    println!("wrote a version-1 repo to {}", gitdir.display());

    // The remote becomes exactly that v1 repo, as a v1 browser left it.
    let pushed = Command::new("git")
        .arg("-C")
        .arg(&gitdir)
        .args(["-c", &format!("http.extraHeader=Authorization: {auth}")])
        .args(["push", "--quiet", "--force", "--mirror", url])
        .status()?;
    if !pushed.success() {
        return Err("git push --mirror failed".into());
    }
    println!("the remote is the version-1 repo (git push --force --mirror)");

    // Now as the browser opening it: migrate, then sync.
    repo.set_remote(&RemoteUrl::new(url.as_str())?)?;
    let migrated = layout::migrate(&mut repo)?;
    println!("migrated: {migrated:?}");
    let ad = host.advertise("git-upload-pack")?;
    match repo.fetch_request(&ad)? {
        FetchRequest::HaveAll => repo.fetch_finish(&ad, None)?,
        FetchRequest::Want(body) => {
            let response = host.post("git-upload-pack", &body)?;
            repo.fetch_finish(&ad, Some(&response))?;
        }
    }
    let outcomes = layout::integrate(
        &mut repo,
        &sig("Alex", 1_800_000_000, 0),
        None,
        &mut |_, p, b, o, t| by_lines(p, b, o, t),
    )?;
    println!("integrate: {outcomes:?}");
    println!("status before the push: {:?}", repo.remote_status()?);
    let ad = host.advertise("git-receive-pack")?;
    match repo.push_request(&ad)? {
        PushRequest::Send(body) => {
            let response = host.post("git-receive-pack", &body)?;
            println!("push: {:?}", repo.push_finish(&body, &response)?);
        }
        other => println!("push: {other:?}"),
    }
    let ad = host.advertise("git-receive-pack")?;
    println!("pushing again: {:?}", repo.push_request(&ad)?);
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
