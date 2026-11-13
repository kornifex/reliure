//! The tables pass: a table of 5 columns or more becomes one block per row, as e-readers can't
//! show it as a grid. Narrower tables stay tables.
use pulldown_cmark::{Event, Tag, TagEnd};

/// The number of columns from which a table becomes one block per row.
const WIDE_TABLE_COLUMNS: usize = 5;

/// The inline events of a table cell's content, empty for an empty cell.
type Cell<'a> = Vec<Event<'a>>;

/// Turns the tables of 5 columns or more into one `div.table-row` block per body row: the row's
/// first cell as a bold title, then one `Header: value` paragraph per other non-empty cell.
pub(super) fn rewrite(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(events.len());
    let mut events = events.into_iter();
    while let Some(event) = events.next() {
        match event {
            Event::Start(Tag::Table(alignments)) if alignments.len() >= WIDE_TABLE_COLUMNS => {
                let mut rows = read_rows(&mut events).into_iter();
                let headers = rows.next().unwrap_or_default();
                for row in rows {
                    push_row_block(&mut out, &headers, row);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Reads a table's rows, header row first, from the events following its start up to its end.
fn read_rows<'a>(events: impl Iterator<Item = Event<'a>>) -> Vec<Vec<Cell<'a>>> {
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    for event in events {
        match event {
            Event::Start(Tag::TableHead | Tag::TableRow) => rows.push(Vec::new()),
            Event::Start(Tag::TableCell) => {
                if let Some(row) = rows.last_mut() {
                    row.push(Vec::new());
                }
            }
            Event::End(TagEnd::TableHead | TagEnd::TableRow | TagEnd::TableCell) => {}
            Event::End(TagEnd::Table) => break,
            inline => {
                if let Some(cell) = rows.last_mut().and_then(|row| row.last_mut()) {
                    cell.push(inline);
                }
            }
        }
    }
    rows
}

/// Pushes the block of one body row, or nothing if all its cells are empty. A value under an
/// empty header goes without a label.
fn push_row_block<'a>(out: &mut Vec<Event<'a>>, headers: &[Cell<'a>], row: Vec<Cell<'a>>) {
    if row.iter().all(Vec::is_empty) {
        return;
    }
    out.push(Event::Html("<div class=\"table-row\">\n".into()));
    let mut cells = row.into_iter();
    if let Some(title) = cells.next().filter(|cell| !cell.is_empty()) {
        out.push(Event::Html("<p class=\"table-row-title\">".into()));
        push_strong(out, title);
        out.push(Event::Html("</p>\n".into()));
    }
    for (header, value) in headers.iter().skip(1).zip(cells) {
        if value.is_empty() {
            continue;
        }
        out.push(Event::Start(Tag::Paragraph));
        if !header.is_empty() {
            push_strong(out, header.iter().cloned().chain([Event::Text(":".into())]));
            out.push(Event::Text(" ".into()));
        }
        out.extend(value);
        out.push(Event::End(TagEnd::Paragraph));
    }
    out.push(Event::Html("</div>\n".into()));
}

fn push_strong<'a>(out: &mut Vec<Event<'a>>, content: impl IntoIterator<Item = Event<'a>>) {
    out.push(Event::Start(Tag::Strong));
    out.extend(content);
    out.push(Event::End(TagEnd::Strong));
}

#[cfg(test)]
mod tests {
    use crate::lang::Lang;
    use crate::render::{BoundNotes, to_xhtml};

    #[test]
    fn a_wide_table_becomes_one_block_per_row() {
        let markdown = "\
| Name | Class | Level | Weapon | Motto |
|---|---|---|---|---|
| Aria | Rogue | 3 | Dagger | Quick |
| Brom | Fighter | 5 | Axe | Strong |
";
        assert_eq!(
            to_xhtml(markdown, Lang::En, &BoundNotes::default()),
            "<div class=\"table-row\">\n\
             <p class=\"table-row-title\"><strong>Aria</strong></p>\n\
             <p><strong>Class:</strong> Rogue</p>\n\
             <p><strong>Level:</strong> 3</p>\n\
             <p><strong>Weapon:</strong> Dagger</p>\n\
             <p><strong>Motto:</strong> Quick</p>\n\
             </div>\n\
             <div class=\"table-row\">\n\
             <p class=\"table-row-title\"><strong>Brom</strong></p>\n\
             <p><strong>Class:</strong> Fighter</p>\n\
             <p><strong>Level:</strong> 5</p>\n\
             <p><strong>Weapon:</strong> Axe</p>\n\
             <p><strong>Motto:</strong> Strong</p>\n\
             </div>\n"
        );
    }

    #[test]
    fn a_table_of_four_columns_stays_a_table() {
        let markdown = "\
| Name | Class | Level | Weapon |
|---|---|---|---|
| Aria | Rogue | 3 | Dagger |
";
        assert_eq!(
            to_xhtml(markdown, Lang::En, &BoundNotes::default()),
            "<table><thead><tr><th>Name</th><th>Class</th><th>Level</th><th>Weapon</th></tr></thead>\
             <tbody>\n<tr><td>Aria</td><td>Rogue</td><td>3</td><td>Dagger</td></tr>\n</tbody></table>\n"
        );
    }

    #[test]
    fn empty_cells_are_left_out() {
        let markdown = "\
| Name | Class | Level | Weapon | Motto |
|---|---|---|---|---|
| Aria |  | 3 |  | Quick |
|  | Fighter | | | |
|  |  | | | |
";
        assert_eq!(
            to_xhtml(markdown, Lang::En, &BoundNotes::default()),
            "<div class=\"table-row\">\n\
             <p class=\"table-row-title\"><strong>Aria</strong></p>\n\
             <p><strong>Level:</strong> 3</p>\n\
             <p><strong>Motto:</strong> Quick</p>\n\
             </div>\n\
             <div class=\"table-row\">\n\
             <p><strong>Class:</strong> Fighter</p>\n\
             </div>\n"
        );
    }

    #[test]
    fn cells_keep_their_inline_content() {
        let markdown = "\
| Name | Class | Level | Weapon | Motto |
|---|---|---|---|---|
| *Aria* | [[Rogue]] | `3` | [Dagger](https://example.com) | **Quick** |
";
        let mut notes = BoundNotes::default();
        notes.insert("Rogue", "note-2.xhtml".into(), "");
        assert_eq!(
            to_xhtml(markdown, Lang::En, &notes),
            "<div class=\"table-row\">\n\
             <p class=\"table-row-title\"><strong><em>Aria</em></strong></p>\n\
             <p><strong>Class:</strong> <a href=\"note-2.xhtml\">Rogue</a></p>\n\
             <p><strong>Level:</strong> <code>3</code></p>\n\
             <p><strong>Weapon:</strong> <a href=\"https://example.com\">Dagger</a></p>\n\
             <p><strong>Motto:</strong> <strong>Quick</strong></p>\n\
             </div>\n"
        );
    }

    #[test]
    fn header_cells_keep_their_inline_content() {
        let markdown = "\
| Name | *Class* | [[Level]] | `Weapon` |  |
|---|---|---|---|---|
| Aria | Rogue | 3 | Dagger | Quick |
";
        assert_eq!(
            to_xhtml(markdown, Lang::En, &BoundNotes::default()),
            "<div class=\"table-row\">\n\
             <p class=\"table-row-title\"><strong>Aria</strong></p>\n\
             <p><strong><em>Class</em>:</strong> Rogue</p>\n\
             <p><strong>Level:</strong> 3</p>\n\
             <p><strong><code>Weapon</code>:</strong> Dagger</p>\n\
             <p>Quick</p>\n\
             </div>\n"
        );
    }

    #[test]
    fn a_wide_table_ends_before_the_next_paragraph() {
        let markdown = "\
| Name | Class | Level | Weapon | Motto |
|---|---|---|---|---|
| Aria | Rogue | 3 | Dagger | Quick |

After.
";
        assert_eq!(
            to_xhtml(markdown, Lang::En, &BoundNotes::default()),
            "<div class=\"table-row\">\n\
             <p class=\"table-row-title\"><strong>Aria</strong></p>\n\
             <p><strong>Class:</strong> Rogue</p>\n\
             <p><strong>Level:</strong> 3</p>\n\
             <p><strong>Weapon:</strong> Dagger</p>\n\
             <p><strong>Motto:</strong> Quick</p>\n\
             </div>\n\
             <p>After.</p>\n"
        );
    }
}
