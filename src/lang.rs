use clap::ValueEnum;

/// The language of a book: its EPUB metadata, and the headings reliure adds to it.
#[derive(Clone, Copy, ValueEnum)]
pub enum Lang {
    En,
    Fr,
}

impl Lang {
    /// The BCP 47 tag of the language.
    pub fn tag(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
        }
    }
}
