use crate::utils::{self, test_read_access, test_write_access};
use anyhow::{bail, Result};
use nix::errno::Errno;
use nix::unistd::getcwd;
use oci_spec::runtime::Spec;
use std::fs::{self, read_dir};
use std::path::Path;

////////// ANCHOR: example_hello_world
pub fn hello_world(_spec: &Spec) {
    println!("Hello world");
}
////////// ANCHOR_END: example_hello_world

pub fn validate_readonly_paths(spec: &Spec) {
    let linux = spec.linux().as_ref().unwrap();
    let ro_paths = match linux.readonly_paths() {
        Some(p) => p,
        None => {
            eprintln!("in readonly paths, expected some readonly paths to be set, found none");
            return;
        }
    };

    if ro_paths.is_empty() {
        return;
    }

    // TODO when https://github.com/rust-lang/rust/issues/86442 stabilizes,
    // change manual matching of i32 to e.kind() and match statement
    for path in ro_paths {
        if let std::io::Result::Err(e) = test_read_access(path) {
            let errno = Errno::from_i32(e.raw_os_error().unwrap());
            // In the integration tests we test for both existing and non-existing readonly paths
            // to be specified in the spec, so we allow ENOENT here
            if errno == Errno::ENOENT {
                /* This is expected */
            } else {
                eprintln!(
                    "in readonly paths, error in testing read access for path {path} : {e:?}"
                );
                return;
            }
        } else {
            /* Expected */
        }

        if let std::io::Result::Err(e) = test_write_access(path) {
            let errno = Errno::from_i32(e.raw_os_error().unwrap());
            // In the integration tests we test for both existing and non-existing readonly paths
            // being specified in the spec, so we allow ENOENT, and we expect EROFS as the paths
            // should be read-only
            if errno == Errno::ENOENT || errno == Errno::EROFS {
                /* This is expected */
            } else {
                eprintln!(
                    "in readonly paths, error in testing write access for path {path} : {e:?}"
                );
                return;
            }
        } else {
            eprintln!("in readonly paths, path {path} expected to not be writable, found writable");
            return;
        }
    }
}

pub fn validate_hostname(spec: &Spec) {
    if let Some(expected_hostname) = spec.hostname() {
        if expected_hostname.is_empty() {
            // Skipping empty hostname
            return;
        }
        let actual_hostname = nix::unistd::gethostname().expect("failed to get current hostname");
        let actual_hostname = actual_hostname.to_str().unwrap();
        if actual_hostname != expected_hostname {
            eprintln!(
                "Unexpected hostname, expected: {expected_hostname:?} found: {actual_hostname:?}"
            );
        }
    }
}

// Run argument test recursively for files after base_dir
fn do_test_mounts_recursive(base_dir: &Path, test_fn: &dyn Fn(&Path) -> Result<()>) -> Result<()> {
    let dirs = read_dir(base_dir).unwrap();
    for dir in dirs {
        let dir = dir.unwrap();
        let f_type = dir.file_type().unwrap();
        if f_type.is_dir() {
            do_test_mounts_recursive(dir.path().as_path(), test_fn)?;
        }

        if f_type.is_file() {
            test_fn(dir.path().as_path())?;
        }
    }

    Ok(())
}

pub fn validate_mounts_recursive(spec: &Spec) {
    if let Some(mounts) = spec.mounts() {
        for mount in mounts {
            if let Some(options) = mount.options() {
                for option in options {
                    match option.as_str() {
                        "rro" => {
                            if let Err(e) =
                                do_test_mounts_recursive(mount.destination(), &|test_file_path| {
                                    if utils::test_write_access(test_file_path.to_str().unwrap())
                                        .is_ok()
                                    {
                                        // Return Err if writeable
                                        bail!(
                                            "path {:?} expected to be read-only, found writable",
                                            test_file_path
                                        );
                                    }
                                    Ok(())
                                })
                            {
                                eprintln!("error in testing rro recursive mounting : {e}");
                            }
                        }
                        "rrw" => {
                            if let Err(e) =
                                do_test_mounts_recursive(mount.destination(), &|test_file_path| {
                                    if utils::test_write_access(test_file_path.to_str().unwrap())
                                        .is_err()
                                    {
                                        // Return Err if not writeable
                                        bail!(
                                            "path {:?} expected to be  writable, found read-only",
                                            test_file_path
                                        );
                                    }
                                    Ok(())
                                })
                            {
                                eprintln!("error in testing rro recursive mounting : {e}");
                            }
                        }
                        "rnoexec" => {
                            if let Err(e) = do_test_mounts_recursive(
                                mount.destination(),
                                &|test_file_path| {
                                    if utils::test_file_executable(test_file_path.to_str().unwrap())
                                        .is_ok()
                                    {
                                        bail!("path {:?} expected to be not executable, found executable", test_file_path);
                                    }
                                    Ok(())
                                },
                            ) {
                                eprintln!("error in testing rnoexec recursive mounting: {e}");
                            }
                        }
                        "rexec" => {
                            if let Err(e) = do_test_mounts_recursive(
                                mount.destination(),
                                &|test_file_path| {
                                    if let Err(ee) = utils::test_file_executable(
                                        test_file_path.to_str().unwrap(),
                                    ) {
                                        bail!("path {:?} expected to be executable, found not executable, error: {ee}", test_file_path);
                                    }
                                    Ok(())
                                },
                            ) {
                                eprintln!("error in testing rexec recursive mounting: {e}");
                            }
                        }
                        "rdiratime" => {
                            println!("test_dir_update_access_time: {mount:?}");
                            let rest = utils::test_dir_update_access_time(
                                mount.destination().to_str().unwrap(),
                            );
                            if let Err(e) = rest {
                                eprintln!("error in testing rdiratime recursive mounting: {e}");
                            }
                        }
                        "rnodiratime" => {
                            println!("test_dir_not_update_access_time: {mount:?}");
                            let rest = utils::test_dir_not_update_access_time(
                                mount.destination().to_str().unwrap(),
                            );
                            if let Err(e) = rest {
                                eprintln!("error in testing rnodiratime recursive mounting: {e}");
                            }
                        }
                        "rdev" => {
                            println!("test_device_access: {mount:?}");
                            let rest =
                                utils::test_device_access(mount.destination().to_str().unwrap());
                            if let Err(e) = rest {
                                eprintln!("error in testing rdev recursive mounting: {e}");
                            }
                        }
                        "rnodev" => {
                            println!("test_device_unaccess: {mount:?}");
                            let rest =
                                utils::test_device_unaccess(mount.destination().to_str().unwrap());
                            if rest.is_ok() {
                                // because /rnodev/null device not access,so rest is err
                                eprintln!("error in testing rnodev recursive mounting");
                            }
                        }
                        "rrelatime" => {
                            println!("rrelatime: {mount:?}");
                            if let Err(e) = utils::test_mount_releatime_option(
                                mount.destination().to_str().unwrap(),
                            ) {
                                eprintln!("path expected to be rrelatime, found not rrelatime, error: {e}");
                            }
                        }
                        "rnorelatime" => {
                            println!("rnorelatime: {mount:?}");
                            if let Err(e) = utils::test_mount_noreleatime_option(
                                mount.destination().to_str().unwrap(),
                            ) {
                                eprintln!("path expected to be rnorelatime, found not rnorelatime, error: {e}");
                            }
                        }
                        "rnoatime" => {
                            println!("rnoatime: {mount:?}");
                            if let Err(e) = utils::test_mount_rnoatime_option(
                                mount.destination().to_str().unwrap(),
                            ) {
                                eprintln!(
                                    "path expected to be rnoatime, found not rnoatime, error: {e}"
                                );
                            }
                        }
                        "rstrictatime" => {
                            println!("rstrictatime: {mount:?}");
                            if let Err(e) = utils::test_mount_rstrictatime_option(
                                mount.destination().to_str().unwrap(),
                            ) {
                                eprintln!("path expected to be rstrictatime, found not rstrictatime, error: {e}");
                            }
                        }
                        "rnosymfollow" => {
                            if let Err(e) = utils::test_mount_rnosymfollow_option(
                                mount.destination().to_str().unwrap(),
                            ) {
                                eprintln!("path expected to be rnosymfollow, found not rnosymfollow, error: {e}");
                            }
                        }
                        "rsymfollow" => {
                            if let Err(e) = utils::test_mount_rsymfollow_option(
                                mount.destination().to_str().unwrap(),
                            ) {
                                eprintln!("path expected to be rsymfollow, found not rsymfollow, error: {e}");
                            }
                        }
                        "rsuid" => {
                            if let Err(e) = utils::test_mount_rsuid_option(
                                mount.destination().to_str().unwrap(),
                            ) {
                                eprintln!("path expected to be rsuid, found not rsuid, error: {e}");
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

pub fn validate_seccomp(spec: &Spec) {
    let linux = spec.linux().as_ref().unwrap();
    if linux.seccomp().is_some() {
        if let Err(errno) = getcwd() {
            if errno != Errno::EPERM {
                eprintln!(
                    "'getcwd()' failed with unexpected error code '{errno}', expected  'EPERM'"
                );
            }
        } else {
            eprintln!(
                "'getcwd()' syscall succeeded. It was expected to fail due to seccomp policies."
            );
        }
    }
}

pub fn validate_sysctl(spec: &Spec) {
    let linux = spec.linux().as_ref().unwrap();
    if let Some(expected_linux_sysctl) = linux.sysctl() {
        for (key, expected_value) in expected_linux_sysctl {
            let key_path = Path::new("/proc/sys").join(key.replace('.', "/"));
            let actual_value = match fs::read(&key_path) {
                Ok(actual_value_bytes) => String::from_utf8_lossy(&actual_value_bytes)
                    .trim()
                    .to_string(),
                Err(e) => {
                    return eprintln!("error due to fail to read the file {key_path:?}, error: {e}")
                }
            };
            if &actual_value != expected_value {
                eprintln!(
                    "Unexpected kernel parameter, expected: {expected_value} found: {actual_value}"
                );
            }
        }
    }
}
