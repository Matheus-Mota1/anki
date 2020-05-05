// Copyright: Ankitects Pty Ltd and contributors
// License: GNU AGPL, version 3 or later; http://www.gnu.org/licenses/agpl.html

use crate::{
    collection::Collection,
    err::{AnkiError, Result},
    notes::NoteID,
    notetype::CardGenContext,
    types::Usn,
};
use itertools::Itertools;
use regex::Regex;
use std::borrow::Cow;

pub struct FindReplaceContext {
    nids: Vec<NoteID>,
    search: Regex,
    replacement: String,
    field_name: Option<String>,
}

impl FindReplaceContext {
    pub fn new(
        nids: Vec<NoteID>,
        search_re: &str,
        repl: impl Into<String>,
        field_name: Option<String>,
    ) -> Result<Self> {
        Ok(FindReplaceContext {
            nids,
            search: Regex::new(search_re).map_err(|_| AnkiError::invalid_input("invalid regex"))?,
            replacement: repl.into(),
            field_name,
        })
    }

    fn replace_text<'a>(&self, text: &'a str) -> Cow<'a, str> {
        self.search.replace_all(text, self.replacement.as_str())
    }
}

impl Collection {
    pub fn find_and_replace(&mut self, ctx: FindReplaceContext) -> Result<u32> {
        self.transact(None, |col| col.find_and_replace_inner(ctx, col.usn()?))
    }

    fn find_and_replace_inner(&mut self, ctx: FindReplaceContext, usn: Usn) -> Result<u32> {
        let mut total_changed = 0;
        let nids_by_notetype = self.storage.note_ids_by_notetype(&ctx.nids)?;
        for (ntid, group) in &nids_by_notetype.into_iter().group_by(|tup| tup.0) {
            let nt = self
                .get_notetype(ntid)?
                .ok_or_else(|| AnkiError::invalid_input("missing note type"))?;
            let genctx = CardGenContext::new(&nt, usn);
            let field_ord = ctx.field_name.as_ref().and_then(|n| nt.get_field_ord(n));
            for (_, nid) in group {
                let mut note = self.storage.get_note(nid)?.unwrap();
                let mut changed = false;
                match field_ord {
                    None => {
                        // all fields
                        for txt in &mut note.fields {
                            if let Cow::Owned(otxt) = ctx.replace_text(txt) {
                                changed = true;
                                *txt = otxt;
                            }
                        }
                    }
                    Some(ord) => {
                        // single field
                        if let Some(txt) = note.fields.get_mut(ord) {
                            if let Cow::Owned(otxt) = ctx.replace_text(txt) {
                                changed = true;
                                *txt = otxt;
                            }
                        }
                    }
                }
                if changed {
                    self.update_note_inner(&genctx, &mut note)?;
                    total_changed += 1;
                }
            }
        }

        Ok(total_changed)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{collection::open_test_collection, decks::DeckID};

    #[test]
    fn findreplace() -> Result<()> {
        let mut col = open_test_collection();

        let nt = col.get_notetype_by_name("Basic")?.unwrap();
        let mut note = nt.new_note();
        note.fields[0] = "one aaa".into();
        note.fields[1] = "two aaa".into();
        col.add_note(&mut note, DeckID(1))?;

        let nt = col.get_notetype_by_name("Cloze")?.unwrap();
        let mut note2 = nt.new_note();
        note2.fields[0] = "three aaa".into();
        col.add_note(&mut note2, DeckID(1))?;

        let nids = col.search_notes_only("")?;
        let cnt = col.find_and_replace(FindReplaceContext::new(
            nids.clone(),
            "(?i)AAA",
            "BBB",
            None,
        )?)?;
        assert_eq!(cnt, 2);

        let note = col.storage.get_note(note.id)?.unwrap();
        // but the update should be limited to the specified field when it was available
        assert_eq!(&note.fields, &["one BBB", "two BBB"]);

        let note2 = col.storage.get_note(note2.id)?.unwrap();
        assert_eq!(&note2.fields, &["three BBB"]);

        assert_eq!(
            col.storage.field_names_for_notes(&nids)?,
            vec!["Back".to_string(), "Front".into(), "Text".into()]
        );
        let cnt = col.find_and_replace(FindReplaceContext::new(
            nids.clone(),
            "BBB",
            "ccc",
            Some("Front".into()),
        )?)?;
        // still 2, as the caller is expected to provide only note ids that have
        // that field, and if we can't find the field we fall back on all fields
        assert_eq!(cnt, 2);

        let note = col.storage.get_note(note.id)?.unwrap();
        // but the update should be limited to the specified field when it was available
        assert_eq!(&note.fields, &["one ccc", "two BBB"]);

        Ok(())
    }
}
