//! What the blame tests and the blame bench share: a deck's saves, merges
//! and a fixture of a few hundred commits.
#![allow(dead_code)]

use meldweb_git::{
    BranchName, Change, CommitId, Identity, Repo, RepoPath, Side, Signature, Store, Timestamp, Tip,
    WriteCommit,
};
use meldweb_wasm::branches::{self, MergeDone, MergePlan};

pub fn sig(who: &str, seconds: i64) -> Signature {
    Signature {
        who: Identity::new(who, format!("{}@example.com", who.to_lowercase())).unwrap(),
        when: Timestamp::new(seconds, 120).unwrap(),
    }
}

pub const T0: i64 = 1_760_000_000;
pub const DAY: i64 = 86_400;

/// A deck whose card lines are `cards`, one per text line, so a card's line
/// is a line `git blame` can name.
pub fn deck(cards: &[&str]) -> String {
    let mut s = String::from("name = \"Lantern\"\nformat = \"commander\"\ncards = [\n");
    for c in cards {
        match c.split_once("  #") {
            Some((card, comment)) => s.push_str(&format!("  {card},  #{comment}\n")),
            None => s.push_str(&format!("  {c},\n")),
        }
    }
    s.push_str(
        "]\n\n[categories]\nDraw = {}\nLand = {}\nRamp = {}\nSideboard = { type = \"sideboard\" }\n",
    );
    s
}

pub fn branch(stem: &str, name: &str) -> BranchName {
    BranchName::new(format!("decks/{stem}/{name}")).unwrap()
}

/// One save of `text` on `decks/<stem>/<name>`, as the editor makes one.
pub fn save<S: Store>(
    repo: &mut Repo<S>,
    stem: &str,
    name: &str,
    text: &str,
    message: &str,
    who: &str,
    at: i64,
) -> CommitId {
    let b = branch(stem, name);
    let tip = repo.branch_tip(&b).unwrap();
    repo.commit(
        &b,
        tip.map_or(Tip::Unborn, Tip::At),
        &WriteCommit {
            changes: vec![Change::Put {
                path: RepoPath::new(format!("{stem}.deck.toml")).unwrap(),
                bytes: text.as_bytes().to_vec(),
            }],
            message: format!("{message}\n"),
            author: sig(who, at),
            committer: sig(who, at),
        },
    )
    .unwrap()
}

pub fn hex(c: CommitId) -> String {
    c.to_hex()
}

pub fn plan_and_merge<S: Store>(
    repo: &mut Repo<S>,
    from: &str,
    into: &str,
    prefer: Option<Side>,
    at: i64,
) -> CommitId {
    let plan = branches::plan(repo, "lantern", from, into, prefer).unwrap();
    let (i, f) = match &plan {
        MergePlan::Merge { into, from, .. } | MergePlan::Conflict { into, from, .. } => {
            (into.clone(), from.clone())
        }
        other => panic!("not a merge: {other:?}"),
    };
    match branches::merge(
        repo,
        "lantern",
        from,
        into,
        &i,
        &f,
        prefer,
        &sig("Alex", at),
    )
    .unwrap()
    {
        MergeDone::Merged { commit, .. } => CommitId::from_hex(&commit).unwrap(),
        other => panic!("not merged: {other:?}"),
    }
}

/// `count` saves of a hundred-card deck: on main a card's quantity or
/// category, every tenth on the branch `try` its own card, Card 99, and the
/// branch merged into main and brought up to it every thirtieth. The number
/// of commits made.
pub fn fixture<S: Store>(repo: &mut Repo<S>, count: usize) -> usize {
    let file = RepoPath::new("lantern.deck.toml").unwrap();
    let line = |i: usize, q: usize, cat: &str| {
        format!(r#"{{ name = "Card {i}", qty = {q}, in = ["{cat}"] }}"#)
    };
    let text = |qty: &[usize], cat: &[&str]| {
        let lines: Vec<String> = (0..100).map(|i| line(i, qty[i], cat[i])).collect();
        deck(&lines.iter().map(String::as_str).collect::<Vec<_>>())
    };
    let read = |repo: &Repo<S>, name: &str| {
        let tip = repo.branch_tip(&branch("lantern", name)).unwrap().unwrap();
        let t = String::from_utf8(repo.read_file(&tip, &file).unwrap().unwrap()).unwrap();
        let d = chip_decklist::deck::Deck::parse(&t).unwrap();
        let qty: Vec<usize> = d.cards.iter().map(|c| c.qty.get() as usize).collect();
        let cat: Vec<&str> = d
            .cards
            .iter()
            .map(|c| {
                if c.categories[0] == "Ramp" {
                    "Ramp"
                } else {
                    "Draw"
                }
            })
            .collect();
        (qty, cat)
    };
    let first = save(
        repo,
        "lantern",
        "main",
        &text(&[1; 100], &["Ramp"; 100]),
        "lantern: made",
        "Alex",
        T0,
    );
    let mut made = 1;
    branches::create(repo, "lantern", "try", &hex(first)).unwrap();
    for n in 1..count {
        let when = T0 + n as i64 * 600;
        if n % 10 == 0 {
            let (mut qty, cat) = read(repo, "try");
            qty[99] = qty[99] % 4 + 1;
            save(
                repo,
                "lantern",
                "try",
                &text(&qty, &cat),
                &format!("lantern: try {n}"),
                "Bo",
                when,
            );
        } else {
            let (mut qty, mut cat) = read(repo, "main");
            let card = (n * 37) % 99;
            if n % 3 == 0 {
                cat[card] = if cat[card] == "Ramp" { "Draw" } else { "Ramp" };
            } else {
                qty[card] = qty[card] % 4 + 1;
            }
            save(
                repo,
                "lantern",
                "main",
                &text(&qty, &cat),
                &format!("lantern: save {n}"),
                "Alex",
                when,
            );
        }
        made += 1;
        if n % 30 == 0 {
            plan_and_merge(repo, "try", "main", Some(Side::Theirs), when + 1);
            made += 1;
            let main = repo
                .branch_tip(&branch("lantern", "main"))
                .unwrap()
                .unwrap();
            let try_ = branch("lantern", "try");
            let tip = repo.branch_tip(&try_).unwrap().unwrap();
            repo.update_branch(&try_, Tip::At(tip), &main).unwrap();
        }
    }
    made
}
