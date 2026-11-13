use std::path::Path;

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use reqwest::blocking::multipart::Form;

/// Uploads `file` to a CrossPoint e-reader in File Transfer mode, into `folder` on its SD card,
/// replacing a book of the same name.
///
/// CrossPoint's `POST /upload` takes the file as the `file` field of a multipart form, the
/// destination folder as the `path` query parameter, and `overwrite=1` to replace an existing
/// file. Unlike WebDAV, it marks the Library as stale, so the book shows up.
pub fn upload(host: &str, folder: &str, file: &Path) -> Result<()> {
    let url = Url::parse_with_params(
        &format!("http://{host}/upload"),
        [("path", folder), ("overwrite", "1")],
    )
    .with_context(|| format!("invalid e-reader host: {host}"))?;
    // CrossPoint saves the file under the multipart file name as sent, so a percent-encoded
    // name ("Book%20one.epub", reqwest's default) would end up on the card.
    let form = Form::new()
        .percent_encode_noop()
        .file("file", file)
        .with_context(|| format!("cannot read {}", file.display()))?;
    let response = Client::new()
        .post(url)
        .multipart(form)
        .send()
        .with_context(|| format!("cannot reach {host}: is the e-reader in File Transfer mode?"))?;

    let status = response.status();
    if !status.is_success() {
        // CrossPoint gives the reason in the body; the status alone still reports the failure.
        let reason = response.text().unwrap_or_default();
        bail!(
            "{host} refused {}: {status} {}",
            file.display(),
            reason.trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    use super::*;

    /// A one-request HTTP server on a free local port: it answers `status` and `body`, and
    /// hands back the request it received.
    fn fake_reader(status: &str, body: &str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = listener.local_addr().unwrap().to_string();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            // A multipart body ends with its closing boundary, `--<boundary>--\r\n`.
            while !request.ends_with(b"--\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..read]);
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (host, server)
    }

    #[test]
    fn sends_the_book_to_the_upload_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("Livre d'été.epub");
        fs::write(&book, "EPUB content").unwrap();
        let (host, server) = fake_reader("200 OK", "File uploaded successfully");

        upload(&host, "/Livres JDR", &book).unwrap();

        let request = server.join().unwrap();
        assert!(request.starts_with("POST /upload?path=%2FLivres+JDR&overwrite=1 HTTP/1.1\r\n"));
        assert!(request.contains("name=\"file\"; filename=\"Livre d'été.epub\""));
        assert!(request.contains("EPUB content"));
    }

    #[test]
    fn a_refused_upload_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("Book.epub");
        fs::write(&book, "EPUB").unwrap();
        let (host, server) = fake_reader(
            "400 Bad Request",
            "A folder with this name already exists: Book.epub",
        );

        let error = upload(&host, "/", &book).unwrap_err().to_string();

        server.join().unwrap();
        assert!(
            error.contains("400 Bad Request A folder with this name already exists"),
            "{error}"
        );
    }
}
