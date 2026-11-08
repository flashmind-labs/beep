use super::*;

#[test]
fn measures_each_volume_once() {
    let dir = tempfile::tempdir().unwrap();
    let inner = dir.path().join("inner");
    std::fs::create_dir(&inner).unwrap();
    let missing = dir.path().join("missing");
    let disks = disks(&[dir.path().to_path_buf(), missing, inner]);
    assert_eq!(disks.len(), 1);
    assert_eq!(disks[0].path, dir.path().display().to_string());
    assert!(disks[0].total_bytes > 0);
    assert!(disks[0].free_bytes <= disks[0].total_bytes);
}

#[test]
fn linux_lists_real_writable_disks() {
    let mounts = "/dev/sda2 / ext4 rw,relatime 0 0\n\
        proc /proc proc rw 0 0\n\
        /dev/sda1 /boot/efi vfat rw 0 0\n\
        /dev/loop3 /snap/core squashfs ro 0 0\n\
        /dev/sdb1 /mnt/big\\040disk ext4 rw 0 0\n\
        /dev/sdc1 /mnt/old ext4 ro 0 0\n";
    assert_eq!(
        linux_volumes(mounts),
        [PathBuf::from("/"), PathBuf::from("/mnt/big disk")]
    );
}

#[test]
fn mac_lists_local_writable_volumes() {
    let mount = "/dev/disk3s1s1 on / (apfs, sealed, local, read-only, journaled)\n\
        /dev/disk3s5 on /System/Volumes/Data (apfs, local, journaled, nobrowse)\n\
        /dev/disk5s1 on /Volumes/Big Disk (apfs, local, journaled)\n\
        /dev/disk6s1 on /Volumes/Installer (hfs, local, read-only, noowners)\n\
        //user@host/share on /Volumes/share (smbfs, nodev, nosuid)\n";
    assert_eq!(mac_volumes(mount), [PathBuf::from("/Volumes/Big Disk")]);
}

/// Run on a Windows machine to see its drives measured.
#[cfg(windows)]
#[test]
fn windows_lists_fixed_drives() {
    let drives = other_volumes();
    assert!(drives.iter().any(|d| d == Path::new("C:\\")), "{drives:?}");
    let measured = disks(&drives);
    println!("{measured:?}");
    assert_eq!(measured.len(), drives.len());
}
