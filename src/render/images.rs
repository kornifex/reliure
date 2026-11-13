//! Images: the attachments a page shows with `<img>`, which the book holds as they are.
use std::path::Path;

use super::escaped;
use crate::vault::Attachment;

/// An image a page shows, which the book must hold.
pub struct Image<'v> {
    /// Its file in the vault.
    pub attachment: &'v Attachment,
    /// Its path in the book, which the page's `<img>` points to: `images/` followed by its
    /// `book_name`.
    pub file: String,
    /// Its media type, such as `image/png`.
    pub media_type: &'static str,
}

impl<'v> Image<'v> {
    /// The image `attachment` is, or `None` when it isn't one of the formats readers show:
    /// JPEG, PNG, GIF, WebP or SVG.
    pub(super) fn new(attachment: &'v Attachment) -> Option<Self> {
        Some(Image {
            attachment,
            file: format!("images/{}", book_name(&attachment.path)),
            media_type: media_type(&attachment.path)?,
        })
    }

    /// The `<img>` tag showing the image in a page of the book, with `alt` as its text.
    pub(super) fn tag(&self, alt: &str) -> String {
        format!(
            "<img src=\"{}\" alt=\"{}\"/>",
            escaped(&self.file),
            escaped(alt)
        )
    }
}

/// The name in the book of the image at `path` in the vault: a hash of that path, then the
/// file's name with its case, every character but `A-Z a-z 0-9 . _ -` turned into `_`
/// (`maps/Pasted image 1.png` → `34f2fc97-Pasted_image_1.png`).
///
/// The hash keeps apart the files the name alone would mix up, from two folders or once
/// cleaned up. The name is the same in the zip, the manifest and the page, as it needs no
/// encoding there, nor in the manifest's id epub-builder derives from it; and it keeps the
/// case of the file, as Kindle refuses a reference whose case differs.
fn book_name(path: &Path) -> String {
    let components: Vec<_> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect();
    let hash = fnv1a(components.join("/").as_bytes()) >> 32;
    let name: String = components
        .last()
        .map_or("", |name| name.as_ref())
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' => c,
            _ => '_',
        })
        .collect();
    format!("{hash:08x}-{name}")
}

/// The 64-bit FNV-1a hash of `bytes`: written out here, as it must stay the same from one
/// version of Rust to the next, which the standard library's hasher doesn't promise.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The media type of an image file, from its extension whatever its case.
fn media_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_string_lossy().to_lowercase();
    Some(match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

/// Whether the text after `|` in an embed is an image size, `300` or `300x200`, which readers
/// have no use for as they fit images to their screen.
pub fn is_size(text: &str) -> bool {
    let is_number = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    match text.split_once('x') {
        Some((width, height)) => is_number(width) && is_number(height),
        None => is_number(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_a_width_or_a_width_and_a_height() {
        assert!(is_size("300"));
        assert!(is_size("300x200"));
        assert!(!is_size("x200"));
        assert!(!is_size("300x"));
        assert!(!is_size("Map of 300"));
    }

    #[test]
    fn images_are_recognised_by_their_extension_whatever_its_case() {
        assert_eq!(media_type(Path::new("a/photo.JPG")), Some("image/jpeg"));
        assert_eq!(media_type(Path::new("plan.svg")), Some("image/svg+xml"));
        assert_eq!(media_type(Path::new("file.pdf")), None);
        assert_eq!(media_type(Path::new("README")), None);
    }

    #[test]
    fn the_hash_is_fnv_1a_on_64_bits() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn a_book_name_keeps_the_file_name_safe_for_a_url() {
        assert_eq!(
            book_name(Path::new("maps/Pasted image 1 é%#(x).png")),
            "5b3fd968-Pasted_image_1_____x_.png"
        );
        assert_eq!(book_name(Path::new("maps/Photo.PNG")), "0ec41df3-Photo.PNG");
        assert_eq!(
            book_name(Path::new("maps/Pasted image 1.png")),
            "34f2fc97-Pasted_image_1.png"
        );
    }

    #[test]
    fn files_sharing_a_name_in_different_folders_get_different_book_names() {
        assert_eq!(book_name(Path::new("one/Map.png")), "a10d2e75-Map.png");
        assert_eq!(book_name(Path::new("two/Map.png")), "91a17244-Map.png");
        // Names the clean-up makes alike stay apart too.
        assert_eq!(book_name(Path::new("a b.png")), "e7417f93-a_b.png");
        assert_eq!(book_name(Path::new("a_b.png")), "9a17f6b7-a_b.png");
    }
}
