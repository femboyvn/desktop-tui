use super::*;
use crate::test_support::TestDir;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::thread;

struct Endpoint(BufReader<TcpStream>);

impl Endpoint {
    fn reply(&mut self, line: &str) {
        writeln!(self.0.get_mut(), "{line}\r").unwrap();
    }

    fn expect(&mut self, expected: &str) {
        let mut line = String::new();
        self.0.read_line(&mut line).unwrap();
        assert_eq!(line, format!("{expected}\r\n"));
    }

    fn command(&mut self, command: &str, reply: &str) {
        self.expect(command);
        self.reply(reply);
    }

    fn data(&mut self, command: &str, reply: &str) -> TcpStream {
        self.expect("PASV");
        let listener = self.passive_listener();
        self.command(command, reply);
        Self::accept_data(listener)
    }

    fn passive_listener(&mut self) -> TcpListener {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        // An address from the PASV reply must never redirect the client away from the control peer.
        self.reply(&format!("227 Entering Passive Mode (192,0,2,9,{},{}).", port >> 8, port & 255));
        listener
    }

    fn accept_data(listener: TcpListener) -> TcpStream {
        let data = listener.accept().unwrap().0;
        data.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        data.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        data
    }

    fn send(&mut self, command: &str, contents: &[u8], completion: &str) {
        self.data(command, "150 Sending").write_all(contents).unwrap();
        self.reply(completion);
    }
}

fn endpoint(script: impl FnOnce(&mut Endpoint) + Send + 'static) -> (Site, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let socket = listener.accept().unwrap().0;
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut server = Endpoint(BufReader::new(socket));
        server.reply("220 Ready");
        server.command("USER anonymous", "331 Password required");
        server.command("PASS anonymous@", "230 Logged in");
        server.command("TYPE I", "200 Binary type selected");
        server.command("PWD", "257 \"/incoming\" current directory");
        script(&mut server);
    });
    let site = Site {
        name: "fixture".into(), protocol: Protocol::Ftp, host: "127.0.0.1".into(),
        port: Some(port), username: String::new(), local_folder: PathBuf::new(),
        remote_folder: PathBuf::new(),
    };
    (site, handle)
}

#[test]
fn passive_binary_roundtrip_preserves_names_and_uses_server_rename() {
    const PAYLOAD: &[u8] = b"a\0b\r\n\xff\nc";
    let (site, server) = endpoint(|s| {
        s.command("CWD /incoming", "250 Changed");
        s.send("MLSD", b"type=cdir; .\r\ntype=pdir; ..\r\ntYpE=file;SIZE=8;  -file [1].bin\r\ntype=dir; folder\r\n", "226 Listed");
        s.command("MKD /incoming/.stage", "257 Created");
        let mut stored = Vec::new();
        s.data("STOR /incoming/.stage/data", "150 Receiving").read_to_end(&mut stored).unwrap();
        assert_eq!(stored, PAYLOAD);
        s.reply("226 Stored");
        s.command("RNFR /incoming/.stage/data", "350 Ready");
        s.command("RNTO /incoming/ -file [1].bin", "250 Renamed");
        s.send("RETR /incoming/ -file [1].bin", &stored, "226 Sent");
        s.command("DELE /incoming/ -file [1].bin", "250 Deleted");
        s.command("RMD /incoming/.stage", "250 Removed");
    });
    let ctl = Control::for_test();
    let (mut client, cwd) = connect(&site, &ctl).unwrap();
    assert_eq!(cwd, Path::new("/incoming"));
    let entries = client.list(&cwd, &ctl).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, OsStr::new(" -file [1].bin"));
    assert!(matches!(entries[0].kind, Kind::File));
    assert_eq!(entries[0].size, Some(8));
    assert!(matches!(entries[1].kind, Kind::Directory));
    let stage = cwd.join(".stage");
    let data = stage.join("data");
    let name = cwd.join(&entries[0].name);
    let tmp = TestDir::new();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");
    std::fs::write(&source, PAYLOAD).unwrap();
    std::fs::write(&target, []).unwrap();
    client.mkdir(&stage, &ctl).unwrap();
    client.upload(&source, &data, &ctl).unwrap();
    client.publish(&data, &name, true, &ctl).unwrap();
    client.download(&name, &target, &ctl).unwrap();
    assert_eq!(std::fs::read(target).unwrap(), PAYLOAD);
    client.remove(&name, &ctl).unwrap();
    client.rmdir(&stage, &ctl).unwrap();
    server.join().unwrap();
}

#[test]
fn legacy_listing_and_conflict_check_preserve_the_destination() {
    const LISTING: &[u8] = b"total 3\r\n-rw-r--r-- 1 a b 17 Jan 12 2020  file two\r\nlrwxrwxrwx 1 a b 5 Jan 12 2020 link -> other\r\n04-08-14  03:09PM  <DIR> dos folder\r\n";
    let (site, server) = endpoint(|s| {
        s.command("CWD /incoming", "250 Changed");
        drop(s.data("MLSD", "502 Command not implemented"));
        s.send("LIST", LISTING, "226 Listed");
        s.command("CWD /incoming", "250 Changed");
        s.send("LIST", LISTING, "226 Listed");
        s.command("CWD /incoming", "250 Changed");
        s.send("LIST", b"unrecognized listing\r\n", "226 Listed");
    });
    let ctl = Control::for_test();
    let (mut client, cwd) = connect(&site, &ctl).unwrap();
    let entries = client.list(&cwd, &ctl).unwrap();
    assert_eq!(entries[0].name, OsStr::new(" file two"));
    assert_eq!(entries[0].size, Some(17));
    assert!(matches!(entries[1].kind, Kind::Other));
    assert!(matches!(entries[2].kind, Kind::Directory));
    assert!(client.publish(&cwd.join("source"), &cwd.join(" file two"), false, &ctl).is_err());
    assert!(client.list(&cwd, &ctl).is_err());
    server.join().unwrap();
}

#[test]
fn lost_rename_reply_reports_uncertain_outcome() {
    let (site, server) = endpoint(|s| {
        s.command("RNFR /incoming/staged", "350 Ready");
        s.expect("RNTO /incoming/final");
    });
    let ctl = Control::for_test();
    let (mut client, cwd) = connect(&site, &ctl).unwrap();
    let error = client.publish(&cwd.join("staged"), &cwd.join("final"), true, &ctl).unwrap_err();
    assert!(error.to_string().contains("unknown"), "{error:#}");
    server.join().unwrap();
}

#[test]
fn transfer_requires_successful_completion_and_literal_safe_paths() {
    let (site, server) = endpoint(|s| {
        s.send("RETR /incoming/file", b"partial", "426 Connection closed");
        // A stale completion must never be mistaken for the reply to another command.
        let _ = s.0.get_mut().write_all(b"226 Transfer aborted\r\n");
    });
    let ctl = Control::for_test();
    let (mut client, cwd) = connect(&site, &ctl).unwrap();
    assert!(client.remove(&cwd.join("a\r\nb"), &ctl).is_err());
    let tmp = TestDir::new();
    let target = tmp.path().join("partial");
    std::fs::write(&target, []).unwrap();
    assert!(client.download(&cwd.join("file"), &target, &ctl).is_err());
    assert!(client.remove(&cwd.join("file"), &ctl).is_err());
    server.join().unwrap();
}

#[test]
fn canceling_a_stalled_download_releases_the_data_socket_and_consumes_the_reply() {
    let (ready, starting) = mpsc::channel();
    let (site, server) = endpoint(move |s| {
        let mut data = s.data("RETR /incoming/file", "150 Sending");
        data.write_all(b"x").unwrap();
        ready.send(()).unwrap();
        assert_eq!(data.read(&mut [0]).unwrap(), 0);
        s.reply("226 Closed");
        s.command("RMD /incoming/.stage", "250 Removed");
    });
    let ctl = Control::for_test();
    let cancel = ctl.clone();
    let canceler = thread::spawn(move || {
        starting.recv_timeout(Duration::from_secs(5)).unwrap();
        let start = Instant::now();
        while cancel.shared.progress.lock().unwrap().bytes == 0 {
            assert!(start.elapsed() < Duration::from_secs(5), "download did not start");
            thread::sleep(Duration::from_millis(1));
        }
        cancel.cancel();
    });
    let (mut client, cwd) = connect(&site, &ctl).unwrap();
    let tmp = TestDir::new();
    let target = tmp.path().join("target");
    std::fs::write(&target, []).unwrap();
    let error = client.download(&cwd.join("file"), &target, &ctl).unwrap_err();
    assert!(error.to_string().contains("canceled"), "{error:#}");
    client.rmdir(&cwd.join(".stage"), &Control::for_test()).unwrap();
    canceler.join().unwrap();
    server.join().unwrap();
}

#[test]
fn canceling_a_stalled_control_reply_disconnects_the_session() {
    let (ready, starting) = mpsc::channel();
    let (site, server) = endpoint(move |s| {
        s.expect("CWD /incoming");
        ready.send(()).unwrap();
        assert_eq!(s.0.read(&mut [0]).unwrap(), 0);
    });
    let ctl = Control::for_test();
    let cancel = ctl.clone();
    let canceler = thread::spawn(move || {
        starting.recv_timeout(Duration::from_secs(5)).unwrap();
        cancel.cancel();
    });
    let (mut client, cwd) = connect(&site, &ctl).unwrap();
    let error = client.list(&cwd, &ctl).unwrap_err();
    assert!(error.to_string().contains("canceled"), "{error:#}");
    server.join().unwrap();
    canceler.join().unwrap();
}

#[test]
fn refusal_to_replace_never_falls_back_to_deleting_the_existing_file() {
    let (site, server) = endpoint(|s| {
        s.command("RNFR /incoming/stage", "350 Ready");
        s.command("RNTO /incoming/destination", "550 Destination exists");
        s.command("RMD /incoming/empty", "250 Removed");
    });
    let ctl = Control::for_test();
    let (mut client, cwd) = connect(&site, &ctl).unwrap();
    let error = client.publish(&cwd.join("stage"), &cwd.join("destination"), true, &ctl).unwrap_err();
    assert!(error.to_string().contains("refused rename"), "{error:#}");
    client.rmdir(&cwd.join("empty"), &ctl).unwrap();
    server.join().unwrap();
}

#[test]
fn named_login_requests_a_masked_password_and_redacts_server_echoes() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let socket = listener.accept().unwrap().0;
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut s = Endpoint(BufReader::new(socket));
        s.reply("220 Ready");
        s.command("USER backup", "331 Password");
        s.command("PASS example-secret", "530 Invalid password example-secret");
    });
    let site = Site { name: "site".into(), protocol: Protocol::Ftp, host: "127.0.0.1".into(),
        port: Some(port), username: "backup".into(), ..Site::default() };
    let (send, events) = mpsc::channel();
    let ctl = Control::new(send);
    let responder = ctl.clone();
    let answer = thread::spawn(move || {
        let prompt = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(prompt, Event::Prompt { kind: PromptKind::Secret, .. }));
        responder.shared.prompt.lock().unwrap().answer = Some(Answer::Text(Zeroizing::new("example-secret".into())));
        responder.shared.wake.notify_all();
    });
    let Err(error) = connect(&site, &ctl) else { panic!("server refused these credentials") };
    let message = format!("{error:#}");
    assert!(message.contains("530") && !message.contains("example-secret"), "{message}");
    server.join().unwrap();
    answer.join().unwrap();
}

#[test]
fn mlsd_directory_identities_include_the_listing_itself_and_preserve_case() {
    let mut ids = HashMap::new();
    for line in ["unique=A1;type=cdir; /incoming", "type=dir;Unique=A1; alias",
        "TyPe=Dir;unique=a1; other", "unique=0;type=pdir; ..", "type=file;unique=B; file"] {
        machine_entry(line, &mut ids).unwrap();
    }
    assert_eq!(ids, HashMap::from([(".".into(), "A1".into()), ("alias".into(), "A1".into()), ("other".into(), "a1".into())]));
}

// A virtual tree keeps this loopback server entirely independent of host paths and credentials.
fn serve_tree(server: &mut Endpoint) {
    let mut cwd = PathBuf::from("/incoming");
    let mut tree: HashMap<PathBuf, Option<Vec<u8>>> = HashMap::from([(cwd.clone(), None)]);
    let mut pending = None;
    let mut listener = None;
    loop {
        let mut line = String::new();
        if server.0.read_line(&mut line).unwrap() == 0 { break; }
        let line = line.strip_suffix("\r\n").unwrap();
        let (verb, argument) = line.split_once(' ').unwrap_or((line, ""));
        let path = Path::new(argument);
        match verb {
            "CWD" => {
                assert!(matches!(tree.get(path), Some(None)));
                cwd = path.to_owned();
                server.reply("250 Changed");
            }
            "PASV" => listener = Some(server.passive_listener()),
            "MKD" => {
                assert!(matches!(tree.get(path.parent().unwrap()), Some(None)));
                assert!(tree.insert(path.to_owned(), None).is_none());
                server.reply("257 Created");
            }
            "RNFR" => {
                assert!(matches!(tree.get(path), Some(Some(_))));
                pending = Some(path.to_owned());
                server.reply("350 Ready");
            }
            "RNTO" => {
                let file = tree.remove(&pending.take().unwrap()).unwrap();
                assert!(tree.insert(path.to_owned(), file).is_none());
                server.reply("250 Renamed");
            }
            "RMD" => {
                assert!(!tree.keys().any(|child| child.parent() == Some(path)));
                assert_eq!(tree.remove(path), Some(None));
                server.reply("250 Removed");
            }
            "MLSD" | "RETR" | "STOR" => {
                let mut data = Endpoint::accept_data(listener.take().unwrap());
                server.reply("150 Ready");
                match verb {
                    "MLSD" => {
                        for (path, content) in &tree {
                            if path.parent() != Some(&cwd) { continue; }
                            let facts = content.as_ref().map_or_else(|| "type=dir;".into(), |file| format!("type=file;size={};", file.len()));
                            writeln!(data, "{facts} {}\r", path.file_name().unwrap().to_str().unwrap()).unwrap();
                        }
                    }
                    "RETR" => data.write_all(tree.get(path).unwrap().as_ref().unwrap()).unwrap(),
                    _ => {
                        let mut bytes = Vec::new();
                        data.read_to_end(&mut bytes).unwrap();
                        assert!(tree.insert(path.to_owned(), Some(bytes)).is_none());
                    }
                }
                drop(data);
                server.reply("226 Complete");
            }
            _ => panic!("unexpected FTP command {line}"),
        }
    }
    assert!(!tree.keys().any(|path| path.to_string_lossy().contains(".desktop-transfer-")));
}

#[test]
fn nested_folder_batch_upload_and_download_through_passive_ftp() {
    let (site, server) = endpoint(serve_tree);
    let ctl = Control::for_test();
    let (mut client, cwd) = connect(&site, &ctl).unwrap();
    super::super::tests::round_trip(client.as_mut(), &cwd, &ctl).unwrap();
    drop(client);
    server.join().unwrap();
}

#[test]
fn recursive_download_skips_mlsd_alias_to_an_ancestor_and_copies_its_sibling() {
    let (site, server) = endpoint(|s| {
        s.command("CWD /incoming", "250 Changed");
        s.send("MLSD", b"Type=cdir;Unique=A; .\r\nType=dir;Unique=B; folder\r\n", "226 Listed");
        s.command("CWD /incoming/folder", "250 Changed");
        s.send("MLSD", b"Type=cdir;Unique=B; .\r\nType=dir;Unique=A; alias\r\nType=file;Size=4; keep\r\n", "226 Listed");
        s.send("RETR /incoming/folder/keep", b"kept", "226 Sent");
    });
    let local = TestDir::new();
    let ctl = Control::for_test();
    let (mut client, cwd) = connect(&site, &ctl).unwrap();
    let result = super::super::copy::batch(Side::Remote, vec!["folder".into()], local.path(), &cwd, client.as_mut(), &ctl).unwrap();
    assert_eq!(result, "Copied 1 file(s), skipped 1.");
    let folder = local.path().join("folder");
    assert!(!folder.join("alias").exists());
    assert_eq!(std::fs::read(folder.join("keep")).unwrap(), b"kept");
    server.join().unwrap();
}
