# reliure

Bind Obsidian notes into EPUB books, to read them on an e-reader.

```sh
reliure ~/vault                              # one EPUB per hub marked `book: true`, in the current folder
reliure ~/vault --out ~/Books --lang fr      # French metadata and callout headings
reliure ~/vault --property ereader           # books are the hubs marked `ereader: true`
reliure ~/vault --send crosspoint.local      # also upload them to a CrossPoint e-reader
```

A book is a hub note, a note that lists other notes, with its sections as chapters. The EPUB keeps the links between the notes it binds, and renders callouts as framed blocks. It sticks to plain XHTML and a short stylesheet, so it reads well on small e-ink screens and on firmwares with a basic renderer (KOReader, CrossPoint).

## Install

With [Rust](https://rustup.rs/) 1.87 or later:

```sh
cargo install --git https://github.com/kornifex/reliure
```

## Usage

```
reliure [OPTIONS] <VAULT>
```

| Option | Effect |
|---|---|
| `<VAULT>` | Root folder of the Obsidian vault |
| `--out <DIR>` | Folder the EPUB files are written to (default: current folder) |
| `--property <NAME>` | Checkbox property marking a hub as a book (default: `book`) |
| `--lang <en\|fr>` | Language of the books: EPUB metadata and callout headings (default: `en`) |
| `--send <HOST>` | Also upload each book to a CrossPoint e-reader at this host |
| `--folder <PATH>` | Folder of the e-reader the books go to, with `--send` (default: `/`) |

Each book is written as `<hub name>.epub`, replacing the previous file.

## How a book is made

Mark a hub with the checkbox property:

```markdown
---
book: true
---
The essentials of running a game, from the blog posts I read this year.

## Design
- **[[Three clue rule]]**: three clues for each conclusion.
- [[Node-based design]]

## At the table
- [[Pacing]], with the [[Three clue rule]] in mind.
- [[Three clue rule]] (see Design)
```

This hub becomes a book titled after the hub, with:

- an opening page with the text before the first `##` heading;
- one chapter per `##` section. The chapter opens with the section's text, so the hub's list works as the chapter's table of contents;
- the notes of a chapter after it, one page each. A note enters the book when a list item **starts** with a link to it, bold or italic allowed: here *Three clue rule*, *Node-based design* and *Pacing*. A link inside a sentence, like *Three clue rule* in the *Pacing* item, is a reference and adds nothing;
- each note bound once, under the first chapter that lists it: the last item of *At the table* adds nothing either.

In the pages:

- `[[links]]` to a note of the book lead to its page. Links to notes outside the book keep their text and lose the link;
- callouts become framed blocks headed by their title, or by their kind (`[!tip]` gives "Tip", or "Astuce" with `--lang fr`). An untitled `[!quote]` stays a plain quote;
- note properties (the YAML frontmatter) are left out.

reliure finds notes by name, as Obsidian links do, so note names must be unique across the vault. Hidden folders (`.obsidian`, `.trash`) are skipped.

## Getting the books on a reader

Copy the EPUB files as any other book: over USB, with Calibre, or through the reader's Wi-Fi upload page.

On a [CrossPoint](https://github.com/crosspoint-reader/crosspoint-reader) e-reader, `--send` does it for you. Put the reader in File Transfer mode, then:

```sh
reliure ~/vault --send crosspoint.local --folder /Books
```

Use the IP address shown on the reader's screen if `crosspoint.local` doesn't resolve on your network. Each book replaces the file of the same name, and shows up in the Library the next time it opens. The connection is plain HTTP, as CrossPoint serves it, so send from a network you trust.

## Limits

Not handled yet: embedded images and notes (`![[…]]`), comments (`%%…%%`), highlights (`==…==`) and footnotes. A link to a heading (`[[Note#Heading]]`) leads to the note's page and keeps `#Heading` in its text. Tables are kept, but wide ones are hard to read on a small screen.

## Development

```sh
cargo test
cargo clippy --all-targets
```

## License

[MIT](LICENSE)
