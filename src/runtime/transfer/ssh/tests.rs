use super::*;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;

// Use an isolated replacement for ssh; the real scp still starts its local and
// remote -f/-t peers and enforces the legacy protocol's filename checks.
fn fixture(temp: &Path, sftp_server: Option<&Path>) -> Endpoint {
    let ssh = temp.join("ssh fixture");
    let server = sftp_server.unwrap_or(Path::new("/bin/false"));
    fs::write(&ssh, format!(r#"#!/bin/sh
socket=
master=no
for arg do
    case "$arg" in
        ControlPath=*) socket=${{arg#ControlPath=}} ;;
        -M) master=yes ;;
    esac
    last=$arg
done
if [ "$master" = yes ]; then
    : > "$socket" || exit 1
    exec sleep 600
fi
if [ "$last" = sftp ]; then
    exec '{}'
fi
exec sh -c "$last"
"#, server.display().to_string().replace('\'', "'\\''"))).unwrap();
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
    Endpoint { host: "fixture".into(), username: "fixture".into(), port: None, ssh, scp: "scp".into() }
}

fn round_trip(mut remote: Box<dyn Remote>, root: &Path, local: &Path, ctl: &Control) {
    let name = std::ffi::OsString::from_vec(b"odd ' [brackets] ? $ \\ \xff file".to_vec());
    let target = root.join(&name);
    let stage = root.join(".staging");
    let data = stage.join(&name);
    let source = local.join("local !.source");
    let bytes: Vec<u8> = (0..262153).map(|n| (n % 251) as u8).collect();
    fs::write(&source, &bytes).unwrap();
    remote.mkdir(&stage, ctl).unwrap();
    assert_eq!(fs::metadata(&stage).unwrap().permissions().mode() & 0o777, 0o700);
    remote.upload(&source, &data, ctl).unwrap();
    assert_eq!(remote.publish(&data, &target, false, ctl).unwrap(), Publication::Linked);
    assert_eq!(fs::read(&data).unwrap(), bytes);
    remote.remove(&data, ctl).unwrap();
    assert_eq!(fs::read(&target).unwrap(), bytes);
    let dest = local.join("download");
    fs::write(&dest, []).unwrap();
    remote.download(&target, &dest, ctl).unwrap();
    assert_eq!(fs::read(dest).unwrap(), bytes);
    fs::write(&source, b"replacement").unwrap();
    remote.upload(&source, &data, ctl).unwrap();
    assert!(remote.publish(&data, &target, false, ctl).is_err());
    assert_eq!(fs::read(&data).unwrap(), b"replacement");
    assert_eq!(fs::read(&target).unwrap(), bytes);
    assert_eq!(remote.publish(&data, &target, true, ctl).unwrap(), Publication::Moved);
    assert!(!data.exists());
    assert_eq!(fs::read(&target).unwrap(), b"replacement");
    std::os::unix::fs::symlink(&target, root.join("link")).unwrap();
    let listing = remote.list(root, ctl).unwrap();
    assert!(listing.iter().any(|e| e.name == name && e.kind == Kind::File && e.size == Some(11)));
    assert!(listing.iter().any(|e| e.name == "link" && e.kind == Kind::Other));
    assert!(listing.iter().any(|e| e.name == ".staging" && e.kind == Kind::Directory));
    remote.remove(&target, ctl).unwrap();
    remote.rmdir(&stage, ctl).unwrap();
}

#[test]
#[ignore = "requires installed OpenSSH scp"]
fn legacy_scp_round_trip() {
    let workspace = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let remote = workspace.path().join("remote ' dir");
    fs::create_dir(&remote).unwrap();
    let ctl = Control::for_test();
    let (connection, resolved) = legacy::connect(fixture(local.path(), None), Driver::new().unwrap(), &remote, &ctl).unwrap();
    assert_eq!(resolved, remote);
    round_trip(connection, &remote, local.path(), &ctl);
}

#[test]
#[ignore = "set DESKTOP_TUI_TEST_SFTP_SERVER to a local OpenSSH sftp-server executable"]
fn sftp_subsystem_round_trip() {
    let workspace = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let server = PathBuf::from(std::env::var_os("DESKTOP_TUI_TEST_SFTP_SERVER").unwrap());
    let ctl = Control::for_test();
    let (connection, resolved) = modern::connect(fixture(local.path(), Some(&server)), Driver::new().unwrap(), workspace.path(), &ctl).unwrap();
    assert_eq!(resolved, workspace.path());
    round_trip(connection, &resolved, local.path(), &ctl);
}

#[test]
fn group_reaps_leader_and_terminates_helpers() {
    let workspace = tempfile::tempdir().unwrap();
    let ready = workspace.path().join("ready");
    let mut command = Command::new("sh");
    command.args(["-c", r#"sleep 600 & echo "$!" > "$1"; wait"#, "sh"]).arg(&ready);
    let group = Group::spawn(command).unwrap();
    let leader = group.child.id();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let helper = loop {
        if let Ok(s) = fs::read_to_string(&ready) && let Ok(pid) = s.trim().parse::<u32>() { break pid; }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    };
    drop(group);
    assert!(!Path::new(&format!("/proc/{leader}")).exists());
    loop {
        let status = fs::read_to_string(format!("/proc/{helper}/status")).unwrap_or_default();
        if status.is_empty() || status.lines().any(|s| s.starts_with("State:") && s.contains("zombie")) { break; }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
fn cancel_stalled_subsystem_reaps_connection() {
    let local = tempfile::tempdir().unwrap();
    let endpoint = fixture(local.path(), None);
    let marker = local.path().join("ready");
    fs::write(&endpoint.ssh, format!("#!/bin/sh\necho $$ > '{}'\nexec sleep 600\n", marker.display())).unwrap();
    let ctl = Control::for_test();
    let cancel = ctl.clone();
    let watcher = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        let pid = loop {
            if let Ok(line) = fs::read_to_string(&marker) && let Ok(pid) = line.trim().parse::<u32>() { break pid; }
            if std::time::Instant::now() >= deadline { cancel.cancel(); panic!("SSH fixture did not start"); }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        cancel.cancel();
        pid
    });
    let error = modern::connect(endpoint, Driver::new().unwrap(), Path::new("."), &ctl).err().unwrap();
    assert!(format!("{error:#}").contains("canceled"));
    let pid = watcher.join().unwrap();
    assert!(!Path::new(&format!("/proc/{pid}")).exists());
}
