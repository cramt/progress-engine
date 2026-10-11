//! Every golden file's trees, read again from its Oracle text. A changed tree
//! fails here until someone has read it and blessed it with
//! `UPDATE_GOLDEN=1 cargo test -p vivisurgeons-insight --test golden`.

use std::fs;

use vivisurgeons_insight::golden;

#[test]
fn pinned_trees_have_not_moved() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/golden");
    let bless = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut moved = Vec::new();
    for entry in fs::read_dir(dir).expect("golden/ exists") {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        let pinned = golden::read(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for (e, trees) in &pinned {
            let now = e.trees();
            if &now != trees {
                moved.push(format!(
                    "{} ({}):\n  pinned {trees:?}\n  now    {now:?}",
                    e.card, e.face
                ));
            }
        }
        if bless {
            let entries: Vec<_> = pinned.into_iter().map(|(e, _)| e).collect();
            fs::write(&path, golden::render(&entries)).unwrap();
        }
    }
    assert!(
        bless || moved.is_empty(),
        "{} pinned faces read differently now; read each, then bless with UPDATE_GOLDEN=1:\n\n{}",
        moved.len(),
        moved.join("\n\n")
    );
}
