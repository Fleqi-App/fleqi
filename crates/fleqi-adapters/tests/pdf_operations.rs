use fleqi_adapters::{capabilities::FileCapabilities, pdf_operations};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
fn execute(
    operation: &str,
    source: &Path,
    dir: &Path,
    params: BTreeMap<String, String>,
) -> Result<String, String> {
    pdf_operations::execute(
        operation,
        &[source.to_owned()],
        dir,
        &params,
        &AtomicBool::new(false),
    )
}
fn generated(text: &str) -> PathBuf {
    PathBuf::from(
        text.lines()
            .find_map(|line| line.strip_prefix("已生成："))
            .unwrap(),
    )
}
#[test]
fn metadata_and_page_count_use_document_properties() {
    let dir = tempfile::tempdir().unwrap();
    let source = FileCapabilities::new()
        .generate_test_pdf(dir.path(), "source.pdf", 2)
        .unwrap();
    let empty = execute("CAP-PDF-007", &source, dir.path(), BTreeMap::new()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&empty).unwrap()["author"],
        serde_json::Value::Null
    );
    let mut doc = lopdf::Document::load(&source).unwrap();
    let info = doc
        .add_object(lopdf::dictionary! {"Author"=>lopdf::Object::string_literal("Fleqi author")});
    doc.trailer.set("Info", info);
    doc.save(&source).unwrap();
    let author = execute("CAP-PDF-007", &source, dir.path(), BTreeMap::new()).unwrap();
    assert!(author.contains("Fleqi author"));
    let output = generated(
        &execute(
            "CAP-PDF-010",
            &source,
            dir.path(),
            BTreeMap::from([("scope".into(), "all".into())]),
        )
        .unwrap(),
    );
    let result = lopdf::Document::load(output).unwrap();
    assert!(result.trailer.get(b"Info").is_err());
    assert_eq!(result.get_pages().len(), 2);
    assert!(
        lopdf::Document::load(source)
            .unwrap()
            .trailer
            .get(b"Info")
            .is_ok()
    );
}
#[test]
fn encrypt_decrypt_use_secret_references_and_bad_password_fails() {
    let dir = tempfile::tempdir().unwrap();
    let source = FileCapabilities::new()
        .generate_test_pdf(dir.path(), "plain.pdf", 1)
        .unwrap();
    let user = fleqi_application::secrets::store("test PDF secret 复杂".into());
    let owner = fleqi_application::secrets::store("different owner credential".into());
    let args = BTreeMap::from([
        ("password".into(), user.clone()),
        ("ownerPassword".into(), owner.clone()),
        ("printing".into(), "none".into()),
        ("allowExtract".into(), "false".into()),
    ]);
    let output = generated(&execute("CAP-PDF-012", &source, dir.path(), args).unwrap());
    assert!(lopdf::Document::load(&output).unwrap().is_encrypted());
    let wrong = fleqi_application::secrets::store("not-the-secret".into());
    let failure = execute(
        "CAP-PDF-011",
        &output,
        dir.path(),
        BTreeMap::from([("password".into(), wrong.clone())]),
    )
    .unwrap_err();
    assert!(!failure.contains("not-the-secret"));
    let restored = generated(
        &execute(
            "CAP-PDF-011",
            &output,
            dir.path(),
            BTreeMap::from([("password".into(), user.clone())]),
        )
        .unwrap(),
    );
    let pdf = lopdf::Document::load(restored).unwrap();
    assert!(!pdf.is_encrypted());
    assert_eq!(pdf.get_pages().len(), 1);
    for reference in [user, owner, wrong] {
        fleqi_application::secrets::release(&reference);
        assert!(fleqi_application::secrets::resolve(&reference).is_err());
    }
}
