//! Edits to a `.deck.toml` that leave the rest of the file as it was written:
//! comments, spacing and order survive, so an edit is the diff it looks like.
//!
//! Every edit re-reads the result with [`Deck::parse`] and refuses what that
//! refuses, so no edit can leave behind a deck the format does not allow.

use thiserror::Error;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

use crate::deck::{CategoryType, Deck, DeckError};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EditError {
    #[error("not a deck file: {0}")]
    Toml(String),
    #[error("there is no card {0}")]
    NoCard(usize),
    #[error("category {name:?} is already declared as {existing}")]
    DeclaredDifferently { name: String, existing: String },
    #[error(transparent)]
    Invalid(#[from] DeckError),
}

fn document(text: &str) -> Result<DocumentMut, EditError> {
    text.parse::<DocumentMut>()
        .map_err(|e| EditError::Toml(e.to_string()))
}

fn finish(doc: DocumentMut) -> Result<String, EditError> {
    let text = doc.to_string();
    Deck::parse(&text)?;
    Ok(text)
}

/// Replaces card `index`'s categories (0-based, in file order). No categories
/// drops its `in` key.
pub fn set_categories(
    text: &str,
    index: usize,
    categories: &[String],
) -> Result<String, EditError> {
    let mut doc = document(text)?;
    let card = doc
        .get_mut("cards")
        .and_then(Item::as_array_mut)
        .and_then(|cards| cards.get_mut(index))
        .and_then(Value::as_inline_table_mut)
        .ok_or(EditError::NoCard(index))?;
    if categories.is_empty() {
        card.remove("in");
    } else {
        let list: Array = categories.iter().map(String::as_str).collect();
        match card.get_mut("in") {
            // In place, so the key keeps its position and spacing.
            Some(existing) => {
                let decor = existing.decor().clone();
                *existing = Value::Array(list);
                *existing.decor_mut() = decor;
            }
            None => {
                card.insert("in", Value::Array(list));
            }
        }
        card.fmt();
    }
    finish(doc)
}

/// Declares `name` under `[categories]`, typed or not. Declaring it again the
/// same way changes nothing.
pub fn declare_category(
    text: &str,
    name: &str,
    kind: Option<CategoryType>,
) -> Result<String, EditError> {
    let mut doc = document(text)?;
    if !doc.contains_key("categories") {
        doc.insert("categories", Item::Table(Table::new()));
    }
    let table = doc["categories"]
        .as_table_like_mut()
        .ok_or_else(|| EditError::Toml("categories is not a table".into()))?;
    if let Some(existing) = table.get(name) {
        let existing_kind = existing
            .as_table_like()
            .and_then(|t| t.get("type"))
            .and_then(Item::as_str)
            .map(str::to_string);
        if existing_kind.as_deref() == kind.map(CategoryType::as_str) {
            return Ok(text.to_string());
        }
        return Err(EditError::DeclaredDifferently {
            name: name.to_string(),
            existing: existing_kind.unwrap_or_else(|| "untyped".into()),
        });
    }
    let mut value = InlineTable::new();
    if let Some(kind) = kind {
        value.insert("type", kind.as_str().into());
        value.fmt();
    }
    table.insert(name, Item::Value(Value::InlineTable(value)));
    finish(doc)
}
