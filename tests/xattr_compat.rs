#[cfg(target_os = "linux")]
#[test]
fn missing_attribute_is_none_for_paths_and_file_descriptors() {
    use xattr::FileExt;
    let file = tempfile::NamedTempFile::new().unwrap();
    assert_eq!(
        xattr::get(file.path(), "user.cintec-missing").unwrap(),
        None
    );
    assert_eq!(
        file.as_file().get_xattr("user.cintec-missing").unwrap(),
        None
    );
    // Other filesystem errors must still propagate.
    assert!(xattr::get(file.path().join("not-a-directory"), "user.cintec-missing").is_err());
}
