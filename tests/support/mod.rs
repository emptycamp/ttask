use std::path::PathBuf;
use tempfile::TempDir;

pub struct StoreScope {
    _tmp: TempDir,
    pub path: PathBuf,
}

impl StoreScope {
    pub fn new() -> Self {
        let tmp = TempDir::new().expect("failed to create tempdir");
        let path = tmp.path().to_path_buf();
        std::env::set_var("TASK_DATA_DIR", &path);
        Self { _tmp: tmp, path }
    }
}

impl Drop for StoreScope {
    fn drop(&mut self) {
        std::env::remove_var("TASK_DATA_DIR");
    }
}

/// Build an assert_cmd Command for the task binary, pointing at an isolated store.
#[macro_export]
macro_rules! task_cmd {
    ($scope:expr) => {{
        let mut cmd = assert_cmd::Command::cargo_bin("ttask").unwrap();
        cmd.env("TASK_DATA_DIR", &$scope.path);
        cmd
    }};
}
