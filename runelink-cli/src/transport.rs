use reqwest::{Client, Url, redirect::Policy};

// A redirect must not downgrade TLS or move credentials to another host.
fn redirect_allowed(previous: &Url, next: &Url) -> bool {
    previous.origin() == next.origin()
}

pub fn build_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("Too many redirects");
            }
            if attempt
                .previous()
                .iter()
                .any(|previous| !redirect_allowed(previous, attempt.url()))
            {
                return attempt.error(
                    "Redirects must stay on the same host, port, and protocol",
                );
            }
            attempt.follow()
        }))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[tokio::test]
    async fn client_rejects_cross_origin_redirects() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let peer = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let bytes_read = socket.read(&mut request).unwrap();
            assert!(bytes_read > 0);
            socket.write_all(b"HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/auth/token\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let error = build_client()
            .unwrap()
            .post(format!("http://{address}/auth/token"))
            .body("password=test")
            .send()
            .await
            .unwrap_err();
        assert!(error.is_redirect());
        peer.join().unwrap();
    }

    #[test]
    fn redirects_cannot_downgrade_or_change_host() {
        let original = Url::parse("https://example.com/auth/token").unwrap();
        for target in [
            "http://example.com/auth/token",
            "https://other.example/auth/token",
            "https://example.com:7001/auth/token",
        ] {
            assert!(!redirect_allowed(&original, &Url::parse(target).unwrap()));
        }
        assert!(redirect_allowed(
            &original,
            &Url::parse("https://example.com/new/token").unwrap()
        ));
    }
}
