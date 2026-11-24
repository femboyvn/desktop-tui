use super::*;
use std::fs;

struct LocalFixture;
impl Remote for LocalFixture {
    fn list(&mut self,path:&Path,ctl:&Control) -> Result<Vec<Item>> { copy::local_list(path,ctl) }
    fn mkdir(&mut self,path:&Path,_:&Control) -> Result<()> { Ok(fs::create_dir(path)?) }
    fn download(&mut self,remote:&Path,local:&Path,_:&Control) -> Result<()> { fs::copy(remote,local)?; Ok(()) }
    fn upload(&mut self,local:&Path,remote:&Path,_:&Control) -> Result<()> { fs::copy(local,remote)?; Ok(()) }
    fn publish(&mut self,from:&Path,to:&Path,replace:bool,_:&Control) -> Result<Publication> {
        ensure!(replace || !to.exists(),"Unexpected overwrite");
        fs::rename(from,to)?; Ok(Publication::Moved)
    }
    fn remove(&mut self,path:&Path,_:&Control) -> Result<()> { Ok(fs::remove_file(path)?) }
    fn rmdir(&mut self,path:&Path,_:&Control) -> Result<()> { Ok(fs::remove_dir(path)?) }
}
pub(super) fn round_trip(remote:&mut dyn Remote, root:&Path, ctl:&Control) -> Result<()> {
    let local = tempfile::tempdir()?; let target = tempfile::tempdir()?;
    let tree = "folder with 'quotes'";
    let file = "binary $[1].bin";
    fs::create_dir_all(local.path().join(tree).join("empty"))?;
    fs::write(local.path().join(tree).join(file),[0,4,255,13,10])?;
    copy::batch(Side::Local,vec![tree.into()],local.path(),root,remote,ctl)?;
    copy::batch(Side::Remote,vec![tree.into()],target.path(),root,remote,ctl)?;
    assert_eq!(fs::read(target.path().join(tree).join(file))?,[0,4,255,13,10]);
    assert!(target.path().join(tree).join("empty").is_dir());
    Ok(())
}
#[test]
fn copy_tree_round_trips_empty_folders_and_binary_files() {
    let root = tempfile::tempdir().unwrap();
    round_trip(&mut LocalFixture,root.path(),&Control::for_test()).unwrap();
}

#[test]
#[ignore = "requires isolated system OpenSSH test-sftp and test-scp aliases serving this machine's temporary directory"]
fn ssh_endpoint_round_trip() {
    for (protocol,host) in [(Protocol::Sftp,"test-sftp"),(Protocol::Scp,"test-scp")] {
        let root = tempfile::tempdir().unwrap();
        let ctl = Control::for_test();
        let site = Site { protocol,host:host.into(),remote_folder:root.path().to_owned(),..Site::default() };
        let (mut remote,path) = ssh::connect(&site,&ctl).unwrap();
        round_trip(remote.as_mut(),&path,&ctl).unwrap_or_else(|e|panic!("{protocol:?}: {e:#}"));
    }
}

#[test]
fn merges_respect_conflict_answer_and_skip_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local"); let remote = dir.path().join("remote");
    for root in [&local,&remote] {fs::create_dir_all(root.join("folder")).unwrap();}
    fs::write(remote.join("folder/data"),"old").unwrap();
    fs::write(remote.join("folder/unrelated"),"keep").unwrap();
    fs::write(local.join("folder/data"),"new").unwrap();
    std::os::unix::fs::symlink("/dev/null",local.join("folder/link")).unwrap();
    for (answer,expected) in [(Answer::Skip,"old"),(Answer::Replace,"new")] {
        let (send,rx) = mpsc::channel();
        let ctl = Control::new(send);
        std::thread::scope(|scope| {
            let job = scope.spawn(||copy::batch(Side::Local,vec!["folder".into()],&local,&remote,&mut LocalFixture,&ctl));
            assert!(matches!(rx.recv_timeout(Duration::from_secs(3)).unwrap(),Event::Prompt {kind:PromptKind::Conflict,..}));
            ctl.shared.prompt.lock().unwrap().answer = Some(answer); ctl.shared.wake.notify_all();
            assert!(job.join().unwrap().is_ok());
        });
        assert_eq!(fs::read_to_string(remote.join("folder/data")).unwrap(),expected);
        assert_eq!(fs::read_to_string(remote.join("folder/unrelated")).unwrap(),"keep");
        assert!(fs::symlink_metadata(remote.join("folder/link")).is_err());
    }
}
#[test]
fn cancellation_while_waiting_preserves_the_destination() {
    let dir = tempfile::tempdir().unwrap(); let remote = dir.path().join("remote"); fs::create_dir(&remote).unwrap();
    fs::write(remote.join("data"),"old").unwrap(); fs::write(dir.path().join("data"),"new").unwrap();
    let (send,rx) = mpsc::channel(); let ctl = Control::new(send);
    std::thread::scope(|scope| {
        let copy = scope.spawn(|| copy::batch(Side::Local,vec!["data".into()],dir.path(),&remote,&mut LocalFixture,&ctl));
        assert!(matches!(rx.recv_timeout(Duration::from_secs(3)).unwrap(),Event::Prompt{..}));
        ctl.cancel(); assert!(copy.join().unwrap().unwrap_err().to_string().contains("canceled"));
    });
    assert_eq!(fs::read_to_string(remote.join("data")).unwrap(),"old");
}
#[test]
fn remote_names_must_be_single_components() {
    use std::os::unix::ffi::OsStrExt;
    for name in ["..",".","", "/root","../other","a/b","file/","nul\0"] {assert!(validate_name(OsStr::new(name)).is_err(),"{name}");}
    for name in [OsStr::new("a.b"),OsStr::from_bytes(b"literal\xff")] {validate_name(name).unwrap();}
}
#[test]
fn saved_sites_are_private_and_stale_updates_preserve_newer_sites() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("sites/sites.json");
    let site = Site {name:"server".into(),host:"example".into(),..Site::default()};
    sites::save(&path,&[],std::slice::from_ref(&site)).unwrap();
    assert_eq!(sites::load(&path).unwrap(),std::slice::from_ref(&site)); assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777,0o600);
    assert!(sites::save(&path,&[],&[]).is_err()); assert_eq!(sites::load(&path).unwrap(),[site]);
    fs::write(&path,b"invalid").unwrap(); assert!(sites::save(&path,&[],&[]).is_err());
    assert_eq!(fs::read(&path).unwrap(),b"invalid");
}
