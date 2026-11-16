use crate::paths::Paths;

pub(crate) fn paths() -> (tempfile::TempDir, Paths) {
    let directory = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", directory.path());
    std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", directory.path().join("root"));
    let paths = Paths::discover().unwrap();
    paths.ensure_management_dirs().unwrap();
    (directory, paths)
}
