# reliure

reliure binds notes of an Obsidian vault into EPUB books, to read them on an e-reader.

## Language

**Vault**:
The Obsidian folder the notes and attachments are read from. A link designates a note or an attachment by its name, or by the end of its path when several files share that name; a link that several files match is ambiguous.

**Hub**:
A note that lists other notes, grouped under `##` sections. A hub marked with the book property is the plan of a book.
_Avoid_: index, MOC, table of contents

**Book**:
The EPUB made from one hub, titled after it.

**Chapter**:
A `##` section of a hub, followed in the book by the notes it binds.
_Avoid_: part, section (outside the hub)

**Bound note**:
A note that has its own page in a book, because a list item of a chapter starts with a link to it. A note is bound once, under the first chapter that lists it.
_Avoid_: included note, chapter note

**Reference**:
A link that binds nothing: inside a sentence, or to a note missing from the vault. It leads to the note's page when that note is bound, and is plain text otherwise.

**Embed**:
A `![[…]]` that puts a note's content, or an attachment, in place where it is written. Embedding a note does not bind it.
_Avoid_: transclusion, include

**Attachment**:
A file of the vault that is not a note, such as an image, found by its name like notes are.
