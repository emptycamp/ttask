fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("expected file path argument");
    if let Ok(content) = std::env::var("FAKE_EDITOR_CONTENT") {
        std::fs::write(&path, content).expect("failed to write editor content");
    }
}
