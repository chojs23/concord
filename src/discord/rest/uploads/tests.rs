use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::Duration,
};

use wreq::header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderValue};

use super::*;
use crate::discord::{
    BASE_ATTACHMENT_LIMIT_BYTES, MessageSendLimits,
    fingerprint::{CLIENT_BUILD_NUMBER, ClientFingerprint},
    rest::{forum::create_forum_post_request_body, messages::message_request_body},
};

#[tokio::test]
async fn attachments_use_cloud_uploads_before_message_and_forum_json() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let directory = tempfile::tempdir().expect("temporary directory is available");
    let path = directory.path().join("disk.txt");
    std::fs::write(&path, b"old").expect("attachment can be written");
    let attachments = [
        MessageAttachmentUpload::from_bytes("memory.txt".to_owned(), b"memory".to_vec()),
        MessageAttachmentUpload::from_path(path.clone(), "disk.txt".to_owned(), 3),
    ];
    std::fs::write(&path, b"disk data").expect("attachment can be updated");

    for forum_post in [false, true] {
        let (base_url, server) = upload_server();
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_static("discord_cookie=private"));
        let rest = DiscordRest::new(
            "private-token".to_owned(),
            wreq::Client::builder()
                .default_headers(headers)
                .timeout(Duration::from_secs(5))
                .build()
                .expect("test API client configuration is valid"),
            Arc::new(ClientFingerprint::new(CLIENT_BUILD_NUMBER)),
        );
        // Only the fixture uses HTTP. Production storage uploads require HTTPS.
        let upload_http = Client::builder()
            .redirect(wreq::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .expect("test upload client configuration is valid");
        let uploaded = rest
            .upload_attachments_to(
                &format!("{base_url}/channels/7/attachments"),
                &attachments,
                BASE_ATTACHMENT_LIMIT_BYTES,
                &upload_http,
            )
            .await
            .expect("both attachments should upload");
        assert_eq!(
            uploaded,
            vec![
                json!({"id": "0", "filename": "memory.txt", "uploaded_filename": "stored/memory.txt"}),
                json!({"id": "1", "filename": "disk.txt", "uploaded_filename": "stored/disk.txt"}),
            ]
        );

        let (endpoint, mut body) = if forum_post {
            (
                "threads",
                create_forum_post_request_body(
                    "Files",
                    "",
                    &[],
                    &attachments,
                    MessageSendLimits::default(),
                )
                .expect("attachment-only forum post is valid"),
            )
        } else {
            ("messages", message_request_body("", Id::new(99), None))
        };
        let message = if forum_post {
            &mut body["message"]
        } else {
            &mut body
        };
        message["attachments"] = Value::Array(uploaded);
        rest.send_json::<Value>(
            rest.raw_http
                .post(format!("{base_url}/channels/7/{endpoint}"))
                .json(&body),
            "send attachments",
        )
        .await
        .expect("message metadata should send after the uploads");

        let requests = server.join().expect("upload server should finish");
        assert!(requests[0].0.starts_with("POST /channels/7/attachments "));
        assert_eq!(requests[0].1[AUTHORIZATION], "private-token");
        assert_eq!(requests[0].1[COOKIE], "discord_cookie=private");
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[0].2).expect("allocation body is JSON"),
            json!({"files": [
                {"id": "0", "filename": "memory.txt", "file_size": 6},
                {"id": "1", "filename": "disk.txt", "file_size": 9},
            ]})
        );
        for (request, (path, bytes)) in requests[1..3].iter().zip([
            ("memory", b"memory".as_slice()),
            ("disk", b"disk data".as_slice()),
        ]) {
            assert!(
                request
                    .0
                    .starts_with(&format!("PUT /{path}?upload_id=secret "))
            );
            assert_eq!(request.2, bytes);
            assert_eq!(request.1["content-type"], "text/plain");
            assert!(request.1.get(AUTHORIZATION).is_none());
            assert!(request.1.get(COOKIE).is_none());
            assert!(request.1.get("x-super-properties").is_none());
        }
        assert!(
            requests[3]
                .0
                .starts_with(&format!("POST /channels/7/{endpoint} "))
        );
        assert_eq!(requests[3].1["content-type"], "application/json");
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[3].2).expect("message body is JSON"),
            body
        );
    }
}

#[tokio::test]
async fn attachment_uploads_recheck_current_file_size() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let directory = tempfile::tempdir().expect("temporary directory is available");
    let path = directory.path().join("changed.bin");
    std::fs::write(&path, [0_u8]).expect("small attachment can be written");
    let attachment = MessageAttachmentUpload::from_path(path.clone(), "changed.bin".to_owned(), 1);
    std::fs::write(
        &path,
        vec![0_u8; (BASE_ATTACHMENT_LIMIT_BYTES + 1) as usize],
    )
    .expect("attachment can be updated");
    let rest = DiscordRest::new(
        "test-token".to_owned(),
        wreq::Client::new(),
        Arc::new(ClientFingerprint::new(CLIENT_BUILD_NUMBER)),
    );
    let error = rest
        .upload_attachments_to(
            "http://127.0.0.1:1/attachments",
            &[attachment],
            BASE_ATTACHMENT_LIMIT_BYTES,
            &Client::new(),
        )
        .await
        .expect_err("actual file size is checked before requesting an upload URL");
    assert!(matches!(error, AppError::AttachmentTooLarge { .. }));
}

type CapturedRequest = (String, HeaderMap, Vec<u8>);

fn upload_server() -> (String, thread::JoinHandle<Vec<CapturedRequest>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("upload server should bind");
    let base_url = format!(
        "http://{}",
        listener.local_addr().expect("upload server has an address")
    );
    let storage_url = base_url.clone();
    let server = thread::spawn(move || {
        let allocation = json!({"attachments": [
            {"id": 1, "upload_url": format!("{storage_url}/disk?upload_id=secret"), "upload_filename": "stored/disk.txt"},
            {"id": "0", "upload_url": format!("{storage_url}/memory?upload_id=secret"), "upload_filename": "stored/memory.txt"},
        ]})
        .to_string();
        let mut requests = Vec::new();
        for body in [allocation.as_str(), "", "", "{}"] {
            let stream = listener.accept().expect("upload server accepts requests").0;
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout can be set");
            let (mut stream, request) = read_request(stream);
            requests.push(request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .expect("upload server writes responses");
        }
        requests
    });
    (base_url, server)
}

fn read_request(stream: TcpStream) -> (TcpStream, CapturedRequest) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .expect("request line is available");
    let mut headers = HeaderMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("headers are available");
        if line == "\r\n" {
            break;
        }
        let (name, value) = line.split_once(':').expect("header has a value");
        headers.insert(
            wreq::header::HeaderName::from_bytes(name.as_bytes()).expect("header name is valid"),
            HeaderValue::from_str(value.trim()).expect("header value is valid"),
        );
    }
    let size = headers["content-length"]
        .to_str()
        .expect("content length is text")
        .parse()
        .expect("content length is numeric");
    let mut body = vec![0; size];
    reader
        .read_exact(&mut body)
        .expect("request body is complete");
    (reader.into_inner(), (request_line, headers, body))
}
