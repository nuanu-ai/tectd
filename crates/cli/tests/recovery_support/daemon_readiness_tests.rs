use super::*;

#[tokio::test]
async fn transient_socket_mode_waits_for_same_identity_before_ready() {
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let parent = fs::symlink_metadata(&root).unwrap();
    let socket = root.join("transient-mode.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o644)).unwrap();
    let before = fs::symlink_metadata(&socket).unwrap();
    let before_identity = (before.dev(), before.ino());
    let before_uid = before.uid();
    let before_mode = before.mode() & 0o777;
    let mut fixture = child("exec /bin/sleep 60", false, false);
    let setter_socket = socket.clone();
    let setter = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(25)).await;
        fs::set_permissions(&setter_socket, fs::Permissions::from_mode(0o600))?;
        fs::symlink_metadata(&setter_socket)
    });
    let ready = Daemon::startup_owned(
        &mut fixture,
        &socket,
        &root,
        (parent.dev(), parent.ino()),
        Duration::from_millis(200),
    )
    .await;
    let live_at_return = fixture.try_wait();
    let ready_metadata = fs::symlink_metadata(&socket);
    let setter_result = setter.await;
    let cleanup = kill_and_reap(&mut fixture).await;
    let reaped = fixture.try_wait();
    let unlink = match &ready {
        Ok(identity) => {
            let mut daemon = Daemon {
                child: fixture,
                socket: socket.clone(),
                inode: *identity,
            };
            daemon.unlink_after_stop()
        }
        Err(_) => Ok(()),
    };
    let removed = !socket.exists();
    drop(listener);
    assert_eq!(cleanup, Ok(()));
    assert!(reaped.unwrap().is_some());
    assert_eq!(unlink, Ok(()));
    assert_eq!(before_uid, rustix::process::geteuid().as_raw());
    assert_eq!(before_mode, 0o644);
    assert_eq!(ready.unwrap(), before_identity);
    assert!(live_at_return.unwrap().is_none());
    let ready_metadata = ready_metadata.unwrap();
    assert_eq!(
        (ready_metadata.dev(), ready_metadata.ino()),
        before_identity
    );
    assert_eq!(ready_metadata.uid(), before_uid);
    assert_eq!(ready_metadata.mode() & 0o777, 0o600);
    let after_setter = setter_result.unwrap().unwrap();
    assert_eq!((after_setter.dev(), after_setter.ino()), before_identity);
    assert_eq!(after_setter.uid(), before_uid);
    assert_eq!(after_setter.mode() & 0o777, 0o600);
    assert!(removed);
}
