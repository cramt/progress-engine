//! Other apps' collection exports, read by header into rows, each shaped as
//! `docs/research/collection-import-formats.md` saw that app write it.

use std::collections::HashMap;

use chip_decklist::collection_import::{read, resolve, Ask, Found, Named, Source};
use chip_decklist::deck::{CardRef, Finish, Printing};

fn printing(set: &str, num: &str) -> Printing {
    Printing {
        set: set.into(),
        num: num.into(),
    }
}

fn dropped(r: &chip_decklist::collection_import::Read) -> Vec<(&str, usize)> {
    r.dropped
        .iter()
        .map(|d| (d.column.as_str(), d.rows))
        .collect()
}

const MANABOX: &str = "Binder Name,Binder Type,Name,Set code,Set name,Collector number,Foil,Rarity,Quantity,ManaBox ID,Scryfall ID,Purchase price,Misprint,Altered,Condition,Language,Purchase price currency\r\n\
Trade binder,binder,\"Brazen Borrower // Petty Theft\",ELD,Throne of Eldraine,39,foil,mythic,1,123,a1b2c3d4-0000-4000-8000-000000000001,12.0,false,false,near_mint,en,USD\r\n\
Trade binder,binder,Clue,TMH2,Modern Horizons 2 Tokens,14,normal,common,3,124,a1b2c3d4-0000-4000-8000-000000000002,0.0,false,false,near_mint,ja,USD\r\n\
Wants,list,Mana Crypt,2XM,Double Masters,270,normal,mythic,1,125,a1b2c3d4-0000-4000-8000-000000000003,0.0,false,false,near_mint,en,USD\r\n";

#[test]
fn a_manabox_export_names_each_card_by_scryfall_id_in_its_binder() {
    let r = read(MANABOX);
    assert_eq!(r.source, Source::ManaBox);
    assert!(r.unreadable.is_empty(), "{:?}", r.unreadable);
    assert_eq!(r.rows.len(), 2);
    let borrower = &r.rows[0];
    assert_eq!(borrower.line, 2);
    assert_eq!(
        borrower.card,
        Named::Id {
            id: "a1b2c3d4-0000-4000-8000-000000000001".into(),
            name: Some("Brazen Borrower // Petty Theft".into()),
        }
    );
    assert_eq!(borrower.finish, Finish::Foil);
    assert_eq!(borrower.place.as_deref(), Some("Trade binder"));
    assert_eq!(r.rows[1].qty.get(), 3);
}

#[test]
fn a_manabox_list_is_no_cards_owned_and_says_so() {
    let r = read(MANABOX);
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].rows, 1);
}

#[test]
fn what_the_collection_keeps_nothing_of_is_counted_by_column() {
    // English is what a card is taken to be, and a zero price or a `false`
    // flag says nothing; the rest is left behind, and the import says so.
    assert_eq!(
        dropped(&read(MANABOX)),
        [("Purchase price", 1), ("Condition", 2), ("Language", 1)]
    );
}

const MOXFIELD: &str = r#""Count","Tradelist Count","Name","Edition","Condition","Language","Foil","Tags","Last Modified","Collector Number","Alter","Proxy","Purchase Price"
"4","0","Sol Ring","cmm","Near Mint","English","","","2025-01-02 10:00:00.000000","400","False","False",""
"1","0","Adrix and Nev, Twincasters","oc21","Good (Lightly Played)","English","etched","","2025-01-02 10:00:00.000000","9","False","False",""
"1","1","Lightning Bolt","2xm","Near Mint","Japanese","foil","","2025-01-02 10:00:00.000000","117","False","False",""
"#;

#[test]
fn a_moxfield_export_names_each_card_by_set_and_number() {
    let r = read(MOXFIELD);
    assert_eq!(r.source, Source::Moxfield);
    assert!(r.unreadable.is_empty(), "{:?}", r.unreadable);
    let finishes: Vec<Finish> = r.rows.iter().map(|r| r.finish).collect();
    assert_eq!(finishes, [Finish::Nonfoil, Finish::Etched, Finish::Foil]);
    assert_eq!(
        r.rows[1].card,
        Named::Printing {
            printing: printing("oc21", "9"),
            name: "Adrix and Nev, Twincasters".into(),
        }
    );
    assert!(r.rows.iter().all(|r| r.place.is_none()));
    assert_eq!(dropped(&r), [("Condition", 3), ("Language", 1)]);
}

const DRAGON_SHIELD: &str = "\"sep=,\"\r\n\
Folder Name,Quantity,Trade Quantity,Card Name,Set Code,Set Name,Card Number,Condition,Printing,Language,Price Bought,Date Bought,LOW,MID,MARKET\r\n\
Box 1,2,0,Delver of Secrets,ISD,Innistrad,51,NearMint,Normal,English,0.25,2021-03-28,0.1,0.2,0.3\r\n\
Rares,1,0,Mana Crypt,SLD,Secret Lair Drop,1537,NearMint,Rainbow Foil,English,100,2021-03-28,90,100,110\r\n\
Rares,1,0,Lightning Bolt,M10,Magic 2010,146,Played,Sparkle,English,1,2021-03-28,1,1,1\r\n";

#[test]
fn a_dragon_shield_export_reads_past_its_separator_line_and_files_by_folder() {
    let r = read(DRAGON_SHIELD);
    assert_eq!(r.source, Source::DragonShield);
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[0].line, 3);
    assert_eq!(r.rows[0].place.as_deref(), Some("Box 1"));
    assert_eq!(
        r.rows[0].card,
        Named::Printing {
            printing: printing("isd", "51"),
            name: "Delver of Secrets".into(),
        }
    );
    assert_eq!(r.rows[1].finish, Finish::Foil);
    // A finish this does not know leaves the card out rather than guess it.
    assert_eq!(r.unreadable.len(), 1);
    assert_eq!(r.unreadable[0].line, 5);
    assert!(
        r.unreadable[0].reason.contains("\"Sparkle\""),
        "{:?}",
        r.unreadable
    );
}

#[test]
fn deckbox_set_codes_are_its_own_so_a_row_without_a_scryfall_id_is_a_name() {
    let text = "Count,Tradelist Count,Name,Edition,Edition Code,Card Number,Condition,Language,Foil,Signed,Artist Proof,Altered Art,Misprint,Promo,Textless,Printing Id,Printing Note,Tags,My Price\n\
2,0,Counterspell,Extras: Duel Decks,ex_127,49,,English,,,,,,,,1234,DDC,,$0.50\n";
    let r = read(text);
    assert_eq!(r.source, Source::Deckbox);
    assert_eq!(r.rows[0].card, Named::Name("Counterspell".into()));
    assert_eq!(r.rows[0].qty.get(), 2);
    assert_eq!(dropped(&r), [("My Price", 1)]);
}

#[test]
fn a_spreadsheet_saved_with_semicolons_is_read_by_its_column_names() {
    let r = read("Name;Set Code;Collector Number;Quantity;Finish\nSol Ring;CMM;400;3;foil\n");
    assert_eq!(r.source, Source::Csv);
    assert_eq!(r.rows[0].qty.get(), 3);
    assert_eq!(r.rows[0].finish, Finish::Foil);
    assert_eq!(
        r.rows[0].card,
        Named::Printing {
            printing: printing("cmm", "400"),
            name: "Sol Ring".into(),
        }
    );
}

#[test]
fn a_text_list_is_read_one_card_a_line() {
    let r = read("4 Sol Ring (CMM) 400 *F*\n1x Island\nnot a card at all (\n");
    assert_eq!(r.source, Source::Text);
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[0].finish, Finish::Foil);
    assert_eq!(r.rows[1].card, Named::Name("Island".into()));
    assert_eq!(r.unreadable.len(), 1, "{:?}", r.rows);
}

#[test]
fn a_row_with_no_quantity_or_none_that_counts_is_refused() {
    let r = read("Name,Quantity\nSol Ring,\nIsland,0\nForest,2\n");
    assert_eq!(r.rows.len(), 1);
    assert_eq!(
        r.unreadable.iter().map(|u| u.line).collect::<Vec<_>>(),
        [2, 3]
    );
}

fn found(entries: &[(Ask, &str, &str, &str)]) -> HashMap<Ask, Found> {
    entries
        .iter()
        .map(|(ask, set, num, name)| {
            (
                ask.clone(),
                Found {
                    printing: printing(set, num),
                    name: (*name).into(),
                },
            )
        })
        .collect()
}

#[test]
fn a_printing_is_pinned_only_where_scryfall_agrees_on_the_name() {
    let r = read(DRAGON_SHIELD);
    // Dragon Shield names a transform card by its front face.
    let answers = found(&[
        (
            Ask::Printing(printing("isd", "51")),
            "isd",
            "51",
            "Delver of Secrets // Insectile Aberration",
        ),
        (
            Ask::Printing(printing("sld", "1537")),
            "sld",
            "1537",
            "Sol Ring",
        ),
    ]);
    let (incoming, notes, unreadable) = resolve(&r, &answers, None);
    assert!(unreadable.is_empty());
    assert_eq!(incoming[0].card, CardRef::Printing(printing("isd", "51")));
    assert_eq!(
        incoming[0].name.as_deref(),
        Some("Delver of Secrets // Insectile Aberration")
    );
    assert_eq!(incoming[1].card, CardRef::Name("Mana Crypt".into()));
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].line, 4);
}

#[test]
fn a_scryfall_id_is_trusted_and_a_row_without_a_place_goes_where_the_import_says() {
    let r = read("Name,Scryfall ID,Quantity\nSol Ring,abc,1\n,def,1\nIsland,,7\n");
    let answers = found(&[(Ask::Id("abc".into()), "cmm", "400", "Sol Ring")]);
    let (incoming, notes, unreadable) = resolve(&r, &answers, Some("Bulk"));
    assert_eq!(incoming.len(), 2);
    assert_eq!(incoming[0].card, CardRef::Printing(printing("cmm", "400")));
    assert_eq!(incoming[1].card, CardRef::Name("Island".into()));
    assert!(incoming.iter().all(|i| i.at.as_deref() == Some("Bulk")));
    assert!(notes.is_empty());
    // An id Scryfall does not know, with no name beside it, names nothing.
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].line, 3);
}

#[test]
fn topdecked_names_its_scryfall_id_a_bare_id() {
    let r = read("QUANTITY,\"NAME\",SETCODE,\"SETNAME\",\"COLLECTOR NUMBER\",FINISH,PRICE,RARITY,ID,ACQUIRED DATE,ACQUIRED PRICE,LANG,PRICE SALE,SIGNING,ALTERATION,CONDITION,NOTES,TAGS\n\
2,\"Sol Ring\",cmm,\"Commander Masters\",\"400\",etched,1.5,uncommon,abc-1,Fri May 15 2026,,en,,,,near mint,,\n");
    assert_eq!(r.source, Source::TopDecked);
    assert_eq!(
        r.rows[0].card,
        Named::Id {
            id: "abc-1".into(),
            name: Some("Sol Ring".into()),
        }
    );
    assert_eq!(r.rows[0].finish, Finish::Etched);
    assert_eq!(r.rows[0].qty.get(), 2);
}

#[test]
fn copies_counted_in_columns_this_cannot_read_refuse_the_file_rather_than_count_one_a_row() {
    // Delver Lens's EchoMTG preset.
    let r = read(
        "Reg Qty,Foil Qty,Name,Set,Acquired,Language\n2,1,Sol Ring,Commander Masters,$0.79,\n",
    );
    assert!(r.rows.is_empty());
    assert_eq!(r.unreadable.len(), 1);
    assert!(
        r.unreadable[0].reason.contains("Reg Qty, Foil Qty"),
        "{:?}",
        r.unreadable
    );
    // A file with no count at all holds one copy a row.
    let r = read("Name,Set Code,Collector Number\nSol Ring,cmm,400\n");
    assert_eq!(r.rows[0].qty.get(), 1);
}
